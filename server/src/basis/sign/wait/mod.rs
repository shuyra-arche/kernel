use crate::basis::unit::task::chain::Drain;
pub(super) mod holder;
pub(super) mod key;
pub(super) mod site;

pub(super) mod block;

use alloc::sync::Arc;
use alloc::sync::Weak;
use core::time::Duration;

use crate::basis::nest::NestId;
use crate::basis::room::deadline;
use crate::basis::room::scheduler::insert;
use crate::basis::room::scheduler::run;
use crate::basis::room::scheduler::slot::current;
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

fn unchain(chain: Option<Drain>) -> impl Iterator<Item = Arc<Task>> {
    chain.into_iter().flatten().inspect(|task| {
        void(task.wait_ticket());
    })
}

fn rise(tasks: impl IntoIterator<Item = Arc<Task>>, place: impl Fn(Arc<Task>)) -> usize {
    let mut woke = 0;
    for task in tasks {
        let _commit = crate::basis::commit();
        let t = task;
        if let Some(join) = t.take_wait() {
            let resumed = join.resume(&t);
            if resumed != Resumption::Complete {
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
/// 谓词与它捕获的上下文（`context` 里强持的 `Nest`/`Reservation`）都在进入
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
    crate::basis::room::scheduler::contract::notify();
    let _commit = crate::basis::commit();
    let chain = {
        let mut sites = sites(key).lock();
        sites.remove(&key)
    };
    let chain = match chain {
        Some(mut site) => {
            fan_out(&site.fwd);
            site.take(true)
        }
        None => None,
    };
    rise(unchain(chain), insert)
}

pub(crate) fn knock(key: WakeKey, life: &Weak<Anchor>) -> usize {
    crate::basis::room::scheduler::contract::notify();
    let _commit = crate::basis::commit();
    let mut fwd = Fwd::empty();
    let chain = {
        let mut sites = sites(key).lock();
        let chain = match sites.get_mut(&key) {
            Some(site) => {
                fwd = site.fwd.clone();
                site.notify(true)
            }
            None => {
                if !Anchor::dead(life)
                    && let Ok(site) = Site::ensure(&mut sites, key, life)
                {
                    site.notify(true);
                }
                None
            }
        };
        prune(&mut sites, key);
        chain
    };
    fan_out(&fwd);
    rise(unchain(chain), insert)
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
    crate::basis::room::scheduler::contract::notify();
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
                site.take(true)
            }
            None => None,
        };
        prune(&mut sites, key);
        chain
    };
    fan_out(&fwd);
    rise(unchain(chain), insert)
}

/// A retained producer can wake existing listeners in fixed batches without
/// allocating a Site. Forwarding observes direct state and has at most eight
/// destinations; Sign keys do not recursively forward their route graph.
pub(crate) fn signal_batch(key: WakeKey) -> bool {
    crate::basis::room::scheduler::contract::notify();
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
    let mut batch = crate::basis::unit::task::chain::Chain::new();
    let more = {
        let mut sites = shard.lock();
        let more = if let Some(site) = sites.get_mut(&key) {
            for _ in 0..4 {
                let Some(task) = site.dequeue() else {
                    break;
                };
                batch.enqueue(task);
            }
            site.has_waiters()
        } else {
            false
        };
        prune(&mut sites, key);
        more
    };
    rise(unchain(batch.take()), insert);
    more
}

pub(crate) fn wipe_space(space: NestId) -> bool {
    let _commit = crate::basis::commit();
    let mut removed = 0usize;
    for shard in 0..SITE_SHARDS {
        loop {
            let taken = {
                let mut sites = shard_at(shard).lock();
                let key = sites
                    .keys()
                    .find(|k| {
                        matches!(
                            k,
                            WakeKey::Reservation { space: s, .. } | WakeKey::Value { space: s, .. }
                                if *s == space
                        )
                    })
                    .copied();
                key.and_then(|key| sites.remove(&key))
            };
            let Some(mut site) = taken else { break };
            rise(unchain(site.take(true)), insert);
            removed += 1;
            if removed == 4 {
                return false;
            }
        }
    }
    true
}

// Wake current listeners without retaining a notification for later listeners.

pub(crate) fn wake_counted(key: WakeKey, all: bool) -> usize {
    let _commit = crate::basis::commit();
    let chain = {
        let mut table = sites(key).lock();
        let chain = table.get_mut(&key).and_then(|site| site.take(all));
        prune(&mut table, key);
        chain
    };
    let mut count = 0;
    for task in unchain(chain) {
        count += 1;
        rise(core::iter::once(task), insert);
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
            let chain = sites.remove(&key).and_then(|mut site| site.take(true));
            drop(sites);
            rise(unchain(chain), insert);
            return false;
        } else {
            let mut beacon_only = false;
            let popped = match sites.get_mut(&key) {
                Some(site) => site.notify(false).and_then(|mut drain| drain.next()),
                None => {
                    beacon_only = true;
                    None
                }
            };
            if beacon_only && let Ok(site) = Site::ensure(&mut sites, key, life) {
                site.notify(false);
            }
            let fwd = sites.get(&key).map_or_else(Fwd::empty, |s| s.fwd.clone());
            prune(&mut sites, key);
            (popped, fwd)
        }
    };
    fan_out(&fwd);
    let Some(task) = popped else { return false };
    void(task.wait_ticket());
    rise(core::iter::once(task), insert);
    true
}

pub fn redeem() -> bool {
    if !deadline::is_owner() {
        return false;
    }
    let Some(_commit) = crate::basis::try_commit() else {
        // Leave both the heap and Holder intact; a retry cannot lose a ticket.
        deadline::retry();
        return false;
    };
    const MAX_DUE: usize = 64;
    let mut due = [0u64; MAX_DUE];
    let n = deadline::drain(clock::now(), &mut due);
    let mut tasks: [Option<Arc<Task>>; MAX_DUE] = [const { None }; MAX_DUE];
    let mut woken = 0usize;
    for (slot, &handle) in tasks.iter_mut().zip(&due[..n]) {
        let Some((key, holder)) = void(Ticket(handle)) else {
            continue;
        };
        let popped = {
            let mut sites = sites(key).lock();
            let w = sites.get_mut(&key).and_then(|site| site.remove(&holder));
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
        crate::basis::room::scheduler::insert,
    ) > 0
}

/// Reserve the Site and exact timeout ticket before changing Task ownership.
fn prepare_wait<'a>(
    commit: &'a crate::basis::CommitGuard,
    task: &Arc<Task>,
    key: WakeKey,
    life: &Weak<Anchor>,
    dur: Duration,
    call: Option<&WaitCall>,
) -> Result<PreparedWait<'a>, ()> {
    #[cfg(debug_assertions)]
    crate::basis::assert_held();
    let result = (|| {
        {
            let mut table = sites(key).lock();
            Site::ensure(&mut table, key, life)?.admit()?;
        }
        let ticket = Ticket::next_id().ok_or(())?;
        let registration = PreparedWait {
            key,
            ticket: Some(ticket),
            _commit: commit,
        };
        let at = call.map_or_else(
            || (dur != Duration::MAX).then(|| clock::now().add(dur).as_ticks()),
            |call| call.deadline(dur),
        );
        if let Some(at) = at {
            hold(ticket, key, task)?;
            deadline::insert(ticket.raw(), at)?;
        }
        Ok(registration)
    })();
    if result.is_err() {
        prune_empty(key);
    }
    result
}

/// Before enrollment this object owns timeout/Holder rollback. Successful
/// enrollment transfers that exact responsibility to Task's waiting state.
#[must_use]
struct PreparedWait<'a> {
    key: WakeKey,
    ticket: Option<Ticket>,
    _commit: &'a crate::basis::CommitGuard,
}
impl PreparedWait<'_> {
    fn ticket(&self) -> Ticket {
        self.ticket.expect("uncommitted wait")
    }
    fn enroll(mut self, task: &Arc<Task>) -> bool {
        let queued = {
            let mut table = sites(self.key).lock();
            let queued = table
                .get_mut(&self.key)
                .is_some_and(|site| site.register(task.clone()));
            prune(&mut table, self.key);
            queued
        };
        if queued {
            self.ticket = None;
        }
        queued
    }
    fn requeue(mut self, task: &Arc<Task>, call: WaitCall) {
        task.block(self.key, self.ticket(), Some(call));
        sites(self.key)
            .lock()
            .get_mut(&self.key)
            .expect("prepared site")
            .requeue(task.clone());
        self.ticket = None;
    }
}
impl Drop for PreparedWait<'_> {
    fn drop(&mut self) {
        if let Some(ticket) = self.ticket.take() {
            void(ticket);
            prune_empty(self.key);
        }
    }
}

pub(crate) fn requeue<E: Copy>(
    task: &Arc<Task>,
    key: WakeKey,
    life: Weak<Anchor>,
    dur: Duration,
    call: WaitCall,
    oom: E,
) -> Result<(), E> {
    let commit = crate::basis::commit();
    let registration =
        prepare_wait(&commit, task, key, &life, dur, Some(&call)).map_err(|()| oom)?;
    registration.requeue(task, call);
    Ok(())
}

pub(crate) fn detach(task: &Arc<Task>) -> bool {
    #[cfg(debug_assertions)]
    crate::basis::assert_held();
    // Task owns the exact wait location. Never search unrelated shards/sites.
    let Some((key, ticket)) = task.state.lock().waiting_on() else {
        return false;
    };
    if ticket == Ticket::LOCAL {
        return crate::basis::room::scheduler::detach_local(task);
    }
    let Some(shard) = site::existing(key) else {
        return false;
    };
    let removed = {
        let mut sites = shard.lock();
        let removed = sites.get_mut(&key).and_then(|site| site.remove(task));
        prune(&mut sites, key);
        removed.is_some()
    };
    if removed {
        void(ticket);
    }
    removed
}

/// Remove an empty source left by a failed wait preparation.
pub(crate) fn prune_empty(key: WakeKey) {
    prune(&mut sites(key).lock(), key);
}
