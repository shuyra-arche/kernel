pub(super) mod fwd;
pub(super) mod holder;
pub(super) mod key;
pub(super) mod site;

pub(super) mod block;
pub(super) mod blocked;
mod counted;
mod fail;
mod unchain;
pub(crate) use counted::wake_counted;

use alloc::sync::{Arc, Weak};
use core::time::Duration;

use crate::basis::memory::nest::Asid;
use crate::basis::work::room::conductor::current;
use crate::basis::work::room::conductor::run::run;
use crate::basis::work::room::deadline;
use crate::basis::work::room::scheduler::launch;
use crate::basis::work::unit::life::{Life, TaskLife};
use crate::basis::work::unit::task::{Resumption, Task};
use crate::machine::clock;

use self::block::block_with_call;
use self::blocked::{Blocked, finish_block};
use self::fail::WaitFail;
use self::fwd::Fwd;
use self::holder::{Ticket, hold, void};
use self::key::WakeKey;
use self::site::{SITE_SHARDS, Site, prune, shard_at, sites, take_beacon};
use self::unchain::Unchain;
use crate::basis::work::room::Handoff;
use crate::basis::work::unit::task::WaitCall;

fn fan_out(fwd: &Fwd) {
    for (id, life) in fwd.entries() {
        knock(WakeKey::Sign { id }, life);
    }
}

fn rise<I: IntoIterator<Item = Arc<Task>>>(tasks: I) -> usize {
    let mut woke = 0;
    for task in tasks {
        let _commit = crate::basis::work::commit();
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
        launch(t);
        woke += 1;
    }
    woke
}

fn alarm_block<E: WaitFail>(task: Arc<Task>, dur: Duration) -> Result<usize, E> {
    let me = task.ident.id;
    let life = task.life();
    drop(task);
    match wait(WakeKey::Alarm { task: me }, life, dur)? {
        Handoff::Switch(pa) => Ok(pa),
        Handoff::Resume(()) => unreachable!("Alarm 无投信方"),
    }
}

pub fn park<E: WaitFail>(duration: Duration) -> Result<usize, E> {
    let Some(task) = current().running_task() else {
        return Ok(run());
    };
    alarm_block::<E>(task, duration)
}

pub fn park_until<E: WaitFail>(at: u64) -> Result<Option<usize>, E> {
    let at_ticks = clock::duration_to_ticks(Duration::from_nanos(at));
    let now_ticks = clock::uptime_ticks();
    if at_ticks <= now_ticks {
        return Ok(None);
    }
    let Some(task) = current().running_task() else {
        return Ok(Some(run()));
    };
    let wait_ticks = at_ticks - now_ticks;
    alarm_block::<E>(task, clock::ticks_to_duration(wait_ticks)).map(Some)
}

pub fn wait<E: WaitFail>(key: WakeKey, life: Weak<Life>, dur: Duration) -> Result<Handoff<()>, E> {
    Ok(finish_block(block_with_call(key, life, dur, None)?))
}

/// 条件等待：在同一提交边界内只跑一次纯谓词，不满足才入队。
///
/// 谓词与它捕获的上下文（`context` 里强持的 `Nest`/`Mapping`）都在进入
/// `finish_block` 之前消费掉：那一步可能切走或进 idle，不能带着它们；
/// 谓词本身必须是纯读——它在这个提交边界内跑，不能推进失效或物化。
/// 醒后不在这里重查：调用方按原样重试那条指令，重查由下一次谓词完成。
pub(crate) fn wait_until<E, C>(
    key: WakeKey,
    life: Weak<Life>,
    context: C,
    ready: fn(&C) -> bool,
) -> Result<Handoff<()>, E>
where
    E: WaitFail,
{
    let blocked = {
        let _commit = crate::basis::work::commit();
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

pub fn fall<E: WaitFail>(me: TaskLife, dur: Duration) -> Result<Handoff<bool>, E> {
    let TaskLife { id, life } = me;
    match wait(WakeKey::Pends { task: id }, life, dur)? {
        Handoff::Switch(pa) => Ok(Handoff::Switch(pa)),
        Handoff::Resume(()) => Ok(Handoff::Resume(true)),
    }
}

pub(crate) fn wipe(key: WakeKey) -> usize {
    let _commit = crate::basis::work::commit();
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
    rise(Unchain::new(chain))
}

pub(crate) fn knock(key: WakeKey, life: &Weak<Life>) -> usize {
    let _commit = crate::basis::work::commit();
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
                if !Life::dead(life)
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
    rise(Unchain::new(chain))
}

pub(crate) fn forward(
    key: WakeKey,
    life: Weak<Life>,
    endpoint: usize,
    endpoint_life: Weak<Life>,
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
    let _commit = crate::basis::work::commit();
    let mut fwd = Fwd::empty();
    let chain = {
        let mut sites = sites(key).lock();
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
    rise(Unchain::new(chain))
}

pub(crate) fn wipe_space(space: Asid) -> usize {
    let _commit = crate::basis::work::commit();
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
                            WakeKey::Space { space: s, .. } | WakeKey::Mapping { space: s, .. } | WakeKey::Value { space: s, .. }
                                if *s == space
                        )
                    })
                    .copied();
                key.and_then(|key| sites.remove(&key))
            };
            let Some(mut site) = taken else { break };
            woken += rise(Unchain::new(site.chain.take()));
        }
    }
    woken
}

pub fn wake(key: WakeKey, life: &Weak<Life>) -> bool {
    let _commit = crate::basis::work::commit();
    debug_assert!(
        !matches!(key, WakeKey::Sign { .. }),
        "组键要走 knock（提示型/整链放行），不能走 wake（交付型/一人）"
    );
    let (popped, fwd) = {
        let mut sites = sites(key).lock();
        if Life::dead(life) {
            let chain = sites.remove(&key).and_then(|mut site| site.chain.take());
            drop(sites);
            rise(Unchain::new(chain));
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
    rise(core::iter::once(task));
    true
}

pub fn redeem() -> bool {
    let _commit = crate::basis::work::commit();
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
    rise(tasks.iter_mut().filter_map(Option::take)) > 0
}

pub(crate) fn requeue<E: Copy>(
    task: &Arc<Task>,
    key: WakeKey,
    life: Weak<Life>,
    dur: Duration,
    call: WaitCall,
    oom: E,
) -> Result<(), E> {
    {
        let mut table = sites(key).lock();
        Site::ensure(&mut table, key, &life).map_err(|()| oom)?;
    }
    let ticket = Ticket::next_id();
    if dur != Duration::MAX {
        hold(ticket, key, task).map_err(|_| oom)?;
        if deadline::tock(ticket.raw(), clock::now().add(dur).as_ticks()).is_err() {
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
