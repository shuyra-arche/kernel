pub(super) mod holder;
pub(super) mod key;
pub(super) mod site;

pub(super) mod block;

use alloc::sync::Arc;
use alloc::sync::Weak;
use core::time::Duration;

use crate::basis::nest::NestId;
use crate::basis::room::conductor::current;
use crate::basis::room::conductor::run::run;
use crate::basis::room::deadline;
use crate::basis::room::scheduler::launch;
use crate::basis::unit::anchor::Anchor;
use crate::basis::unit::task::Resumption;
use crate::basis::unit::task::Task;
use crate::metal::clock;

use self::block::WaitFail;
use self::block::block_with_call;
use self::block::finish_block;
use self::holder::Ticket;
use self::holder::hold;
use self::holder::void;
use self::key::WakeKey;
use self::site::Fwd;
use self::site::SITE_SHARDS;
use self::site::Site;
use self::site::prune;
use self::site::shard_at;
use self::site::sites;
use self::site::take_beacon;
use crate::basis::room::Handoff;
use crate::basis::unit::task::WaitCall;

fn fan_out(fwd: &Fwd) {
    for (id, life) in fwd.entries() {
        knock(WakeKey::Sign { id }, life);
    }
}

struct Unchain {
    cur: Option<Arc<Task>>,
}

impl Unchain {
    fn new(cur: Option<Arc<Task>>) -> Self {
        Self { cur }
    }
}

impl Iterator for Unchain {
    type Item = Arc<Task>;

    fn next(&mut self) -> Option<Arc<Task>> {
        let task = self.cur.take()?;
        void(task.wait_ticket());
        self.cur = task.take_next();
        Some(task)
    }
}

fn rise(tasks: impl IntoIterator<Item = Arc<Task>>, place: impl Fn(Arc<Task>)) -> usize {
    let mut woke = 0;
    for task in tasks {
        let _commit = crate::basis::commit();
        let t = task;
        if let Some(join) = t.take_wait() {
            let resumed = join.resume(&t);
            if resumed == Resumption::Waiting {
                continue;
            }
        }
        if !Task::rise(&t) {
            continue;
        }
        place(t);
        woke += 1;
    }
    woke
}

pub fn park_until<E: WaitFail>(at: u64) -> Result<Option<usize>, E> {
    let deadline = clock::deadline(kernel_api::model::wait::Wait::At(at)).expect("finite");
    loop {
        if clock::now().as_ticks() >= deadline {
            return Ok(None);
        }
        let Some(task) = current().running_task() else {
            return Ok(Some(run()));
        };
        let key = WakeKey::Alarm {
            task: task.ident.id,
        };
        let life = task.life();
        drop(task);
        let blocked = block_with_call::<E>(
            key,
            life,
            clock::remaining(Some(deadline)),
            Some(WaitCall::Sleep { deadline }),
        )?;
        match finish_block(blocked) {
            Handoff::Switch(pa) => return Ok(Some(pa)),
            Handoff::Resume(()) => {}
        }
    }
}

/// 条件等待：在同一提交边界内只跑一次纯谓词，不满足才入队。
///
/// 谓词与它捕获的上下文（`context` 里强持的 `Nest`/`Mapping`）都在进入
/// `finish_block` 之前消费掉：那一步可能切走或进 idle，不能带着它们；
/// 谓词本身必须是纯读——它在这个提交边界内跑，不能推进失效或物化。
/// 醒后不在这里重查：调用方按原样重试那条指令，重查由下一次谓词完成。
pub(crate) fn wait_until<E, C>(
    key: WakeKey,
    life: Weak<Anchor>,
    context: C,
    ready: fn(&C) -> bool,
) -> Result<Handoff<()>, E>
where
    E: WaitFail,
{
    let blocked = {
        let _commit = crate::basis::commit();
        if ready(&context) {
            None
        } else {
            Some(block_with_call::<E>(key, life, Duration::MAX, None)?)
        }
    };
    drop(context);
    Ok(match blocked {
        None => Handoff::Resume(()),
        Some(blocked) => finish_block(blocked),
    })
}

pub(crate) fn wipe(key: WakeKey) -> usize {
    let _commit = crate::basis::commit();
    let chain = {
        let mut sites = sites(key).lock();
        sites.remove(&key)
    };
    let chain = match chain {
        Some(mut site) => {
            fan_out(&site.fwd);
            site.chain.take()
        }
        None => None,
    };
    rise(Unchain::new(chain), launch)
}

pub(crate) fn knock(key: WakeKey, life: &Weak<Anchor>) -> usize {
    let _commit = crate::basis::commit();
    let mut fwd = Fwd::empty();
    let chain = {
        let mut sites = sites(key).lock();
        let chain = match sites.get_mut(&key) {
            Some(site) => {
                fwd = site.fwd.clone();
                let chain = site.chain.take();
                if chain.is_none() {
                    site.pend = true;
                }
                chain
            }
            None => {
                if !Anchor::dead(life)
                    && let Ok(site) = Site::ensure(&mut sites, key, life)
                {
                    site.pend = true;
                }
                None
            }
        };
        prune(&mut sites, key);
        chain
    };
    fan_out(&fwd);
    rise(Unchain::new(chain), launch)
}

pub(crate) fn forward(
    key: WakeKey,
    life: Weak<Anchor>,
    endpoint: usize,
    endpoint_life: Weak<Anchor>,
) -> Result<(), ()> {
    let mut sites = sites(key).lock();
    let site = Site::ensure(&mut sites, key, &life)?;
    let r = site.fwd.attach(endpoint, endpoint_life);
    prune(&mut sites, key);
    r
}

pub(crate) fn unforward(key: WakeKey, endpoint: usize) {
    let mut sites = sites(key).lock();
    if let Some(site) = sites.get_mut(&key) {
        site.fwd.detach(endpoint);
    }
    prune(&mut sites, key);
}

pub(crate) fn signal(key: WakeKey) -> usize {
    let _commit = crate::basis::commit();
    let Some(shard) = site::existing(key) else {
        return 0;
    };
    let mut fwd = Fwd::empty();
    let chain = {
        let mut sites = shard.lock();
        let chain = match sites.get_mut(&key) {
            Some(site) => {
                fwd = site.fwd.clone();
                site.chain.take()
            }
            None => None,
        };
        prune(&mut sites, key);
        chain
    };
    fan_out(&fwd);
    rise(Unchain::new(chain), launch)
}

/// A retained producer can wake existing listeners in fixed batches without
/// allocating a Site. Forwarding observes direct state and has at most eight
/// destinations; Sign keys do not recursively forward their route graph.
pub(crate) fn signal_batch(key: WakeKey) -> bool {
    let _commit = crate::basis::commit();
    let fwd = site::existing(key).and_then(|shard| shard.lock().get(&key).map(|s| s.fwd.clone()));
    let mut more = listeners_batch(key);
    if let Some(fwd) = fwd {
        for (id, _) in fwd.entries() {
            more |= listeners_batch(WakeKey::Sign { id });
        }
    }
    more
}
fn listeners_batch(key: WakeKey) -> bool {
    let Some(shard) = site::existing(key) else {
        return false;
    };
    // Detach the batch before resuming: a waiter that requeues must not consume
    // the rest of this batch and prevent another waiter from making progress.
    let mut batch = crate::basis::room::chain::Chain::new();
    let more = {
        let mut sites = shard.lock();
        let more = if let Some(site) = sites.get_mut(&key) {
            for _ in 0..4 {
                let Some(task) = site.chain.pop() else {
                    break;
                };
                batch.push(task);
            }
            !site.chain.is_empty()
        } else {
            false
        };
        prune(&mut sites, key);
        more
    };
    rise(Unchain::new(batch.take()), launch);
    more
}

pub(crate) fn wipe_space(space: NestId) -> usize {
    let _commit = crate::basis::commit();
    let mut woken = 0usize;
    for shard in 0..SITE_SHARDS {
        loop {
            let taken = {
                let mut sites = shard_at(shard).lock();
                let key = sites
                    .keys()
                    .find(|k| {
                        matches!(
                            k,
                            WakeKey::Mapping { space: s, .. } | WakeKey::Value { space: s, .. }
                                if *s == space
                        )
                    })
                    .copied();
                key.and_then(|key| sites.remove(&key))
            };
            let Some(mut site) = taken else { break };
            woken += rise(Unchain::new(site.chain.take()), launch);
        }
    }
    woken
}

// Wake current listeners without retaining a notification for later listeners.

pub(crate) fn wake_counted(key: WakeKey, all: bool) -> usize {
    let _commit = crate::basis::commit();
    let chain = {
        let mut table = sites(key).lock();
        let chain = table.get_mut(&key).and_then(|site| {
            if all {
                site.chain.take()
            } else {
                site.chain.pop()
            }
        });
        prune(&mut table, key);
        chain
    };
    let mut count = 0;
    for task in Unchain::new(chain) {
        count += 1;
        rise(core::iter::once(task), launch);
    }
    count
}

pub fn wake(key: WakeKey, life: &Weak<Anchor>) -> bool {
    let _commit = crate::basis::commit();
    debug_assert!(
        !matches!(key, WakeKey::Sign { .. }),
        "组键要走 knock（提示型/整链放行），不能走 wake（交付型/一人）"
    );
    let (popped, fwd) = {
        let mut sites = sites(key).lock();
        if Anchor::dead(life) {
            let chain = sites.remove(&key).and_then(|mut site| site.chain.take());
            drop(sites);
            rise(Unchain::new(chain), launch);
            return false;
        } else {
            let mut beacon_only = false;
            let popped = match sites.get_mut(&key) {
                Some(site) => match site.chain.pop() {
                    Some(task) => Some(task),
                    None => {
                        site.pend = true;
                        None
                    }
                },
                None => {
                    beacon_only = true;
                    None
                }
            };
            if beacon_only && let Ok(site) = Site::ensure(&mut sites, key, life) {
                site.pend = true;
            }
            let fwd = sites.get(&key).map_or_else(Fwd::empty, |s| s.fwd.clone());
            prune(&mut sites, key);
            (popped, fwd)
        }
    };
    fan_out(&fwd);
    let Some(task) = popped else { return false };
    void(task.wait_ticket());
    rise(core::iter::once(task), launch);
    true
}

pub fn redeem() -> bool {
    if !deadline::is_owner() {
        return false;
    }
    let _commit = crate::basis::commit();
    const MAX_DUE: usize = 64;
    let mut due = [0u64; MAX_DUE];
    let n = deadline::drain(clock::now(), &mut due);
    let mut tasks: [Option<Arc<Task>>; MAX_DUE] = [const { None }; MAX_DUE];
    let mut woken = 0usize;
    for (slot, &handle) in tasks.iter_mut().zip(&due[..n]) {
        let Some((key, holder)) = void(Ticket(handle)) else {
            continue;
        };
        drop(holder);
        let popped = {
            let mut sites = sites(key).lock();
            let pick = &mut |t: &mut Arc<Task>| t.wait_ticket() == Ticket(handle);
            let w = sites
                .get_mut(&key)
                .and_then(|site| site.chain.remove_if(pick));
            prune(&mut sites, key);
            w
        };
        let Some(w) = popped else { continue };
        *slot = Some(w);
        woken += 1;
    }
    let _ = woken;
    rise(
        tasks.iter_mut().filter_map(Option::take),
        crate::basis::room::scheduler::launch_expired,
    ) > 0
}

pub(crate) fn requeue<E: Copy>(
    task: &Arc<Task>,
    key: WakeKey,
    life: Weak<Anchor>,
    dur: Duration,
    call: WaitCall,
    oom: E,
) -> Result<(), E> {
    {
        let mut table = sites(key).lock();
        Site::ensure(&mut table, key, &life).map_err(|()| oom)?;
    }
    let ticket = Ticket::next_id().ok_or(oom)?;
    if let Some(at) = call.deadline(dur) {
        hold(ticket, key, task).map_err(|_| oom)?;
        if deadline::tock(ticket.raw(), at).is_err() {
            void(ticket);
            return Err(oom);
        }
    }
    task.block(key, ticket, Some(call));
    let mut table = sites(key).lock();
    let site = table.get_mut(&key).expect("requeued site");
    site.pend = false;
    site.push_back(task.clone());
    Ok(())
}

pub(crate) fn detach(task: &Arc<Task>) -> bool {
    for shard in 0..SITE_SHARDS {
        let mut sites = shard_at(shard).lock();
        let mut hit: Option<(WakeKey, Ticket)> = None;
        for (key, site) in sites.iter_mut() {
            if let Some(ticket) = site
                .chain
                .remove_if(&mut |t| Arc::ptr_eq(t, task))
                .map(|node| node.wait_ticket())
            {
                hit = Some((*key, ticket));
                break;
            }
        }
        let out = hit.map(|(key, ticket)| {
            prune(&mut sites, key);
            ticket
        });
        drop(sites);
        if let Some(ticket) = out {
            void(ticket);
            return true;
        }
    }
    false
}

/// Remove an empty source left by a failed wait preparation.
pub(crate) fn prune_empty(key: WakeKey) {
    prune(&mut sites(key).lock(), key);
}
