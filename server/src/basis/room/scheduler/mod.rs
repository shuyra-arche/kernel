//! Scheduling coordination, placement and complete per-Hart dispatch.
pub(crate) mod contract;
pub(crate) mod idle;
pub(crate) mod slot;

use alloc::boxed::Box;
use alloc::sync::Arc;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

use crate::basis::nest::NestError;
use crate::basis::room::scheduler::slot::Slot;

use crate::basis::sign;
use crate::basis::unit::task::Task;
use crate::metal::hart;
use crate::metal::hart::HartId;
use crate::metal::sync::OnceLock;

use self::idle::nudge;
use self::idle::pick;
use self::idle::waiting;
use crate::basis::unit::task::registry;

/// 每核一张的运行槽，按 hart 号排。
static SCHEDULERS: OnceLock<&'static [Slot]> = OnceLock::new();

static STEAL_CURSOR: AtomicUsize = AtomicUsize::new(0);

static KICKS: AtomicUsize = AtomicUsize::new(0);
static FALLBACK: AtomicUsize = AtomicUsize::new(0);

/// 装好全局表：每核一张空槽，并把本核的槽挂进 `PerHart`。
pub(crate) fn init() -> Result<(), NestError> {
    let n = hart::hart_count();
    assert!(n > 0, "no harts");
    let mut sched = Box::<[Slot]>::try_new_uninit_slice(n).map_err(|_| NestError::OutOfMemory)?;
    for (h, slot) in sched.iter_mut().enumerate() {
        slot.write(Slot::new(HartId::new(h)));
    }
    let mut sched = unsafe { sched.assume_init() };
    for (h, c) in sched.iter_mut().enumerate() {
        hart::set_scheduler(HartId::new(h), c as *mut Slot as *mut ());
    }
    assert!(
        SCHEDULERS.set(Box::leak(sched)).is_ok(),
        "schedulers double init"
    );
    Ok(())
}

/// 表装好了没有 —— 装之前本核还没有运行槽，问身份也就只能答“没有”。
pub(crate) fn installed() -> bool {
    SCHEDULERS.get().is_some()
}

fn schedulers() -> &'static [Slot] {
    SCHEDULERS.get().expect("schedulers not initialized")
}
pub(crate) fn isolated(hart: HartId) -> bool {
    SCHEDULERS
        .get()
        .is_some_and(|all| all[hart.get()].isolated.load(Ordering::Acquire))
}
pub(crate) fn detach_local(task: &Arc<Task>) -> bool {
    let Some(contract) = task.schedule.lock().contract() else {
        return false;
    };
    schedulers()[contract.spec.hart as usize].detach_local(task)
}

/// 把一枚就绪任务放到挑中的那颗核上。
pub(crate) fn insert(task: Arc<Task>) {
    let cursor = task
        .ident
        .id
        .get()
        .wrapping_add(task.schedule.lock().placement());
    place(
        pick(cursor, |candidate| task.can_run_on(candidate), true),
        task,
    );
}

pub(crate) fn insert_expired(task: Arc<Task>) {
    // Expiry always runs on the boot Hart. Excluding it when all compatible
    // Harts are busy would pin a Task compatible with only two Harts.
    let cursor = task
        .ident
        .id
        .get()
        .wrapping_add(task.schedule.lock().placement());
    place(
        pick(cursor, |candidate| task.can_run_on(candidate), false),
        task,
    );
}

/// 把一枚就绪任务放到指定核上：接进它那张就绪队列，必要时叫醒它。
fn place(hart: HartId, task: Arc<Task>) {
    assert!(task.can_run_on(hart), "incompatible Task placement");
    let newly_ready = schedulers()[hart.get()].enqueue(task);
    if waiting(hart) || newly_ready {
        notify(hart);
        if !waiting(hart)
            && let Some(idle) = schedulers()
                .iter()
                .find(|s| s.hart != hart && waiting(s.hart))
        {
            notify(idle.hart);
        }
    } else {
        FALLBACK.fetch_add(1, Ordering::Relaxed);
    }
}

fn notify(hart: HartId) {
    KICKS.fetch_add(1, Ordering::Relaxed);
    nudge(hart);
}

/// 从别的核的就绪队列里偷一枚（本核自己那张由 `dequeue` 管）。
fn steal(commit: &crate::basis::CommitGuard) -> Option<Arc<Task>> {
    let all = schedulers();
    let me = hart::hart_id();
    let start = STEAL_CURSOR.fetch_add(1, Ordering::Relaxed) % all.len();
    for offset in 0..all.len() {
        let victim = &all[(start + offset) % all.len()];
        if victim.hart != me
            && let Some(task) = victim.steal(me, commit)
        {
            return Some(task);
        }
    }
    None
}

/// 把一枚任务从任何一张就绪队列里摘掉。
pub(crate) fn remove(target: &Arc<Task>) -> bool {
    let _commit = crate::basis::commit();
    for s in schedulers() {
        if s.remove(target, &_commit) {
            return true;
        }
    }
    false
}

/// 停机拆台：清空每张就绪队列，交还名册里还挂着的东西，再拆掉等待面。
pub(crate) fn rip() {
    let _commit = crate::basis::commit();
    let Some(cs) = SCHEDULERS.get() else { return };
    for c in cs.iter() {
        c.clear(&_commit);
    }
    let mut after = 0;
    while let Some(task) = registry::after(after) {
        after = task.ident.id.get();
        task.ident.team.release_held(&task);
    }
    sign::clear_sources();
    crate::basis::unit::lifecycle::rip();
    sign::clear_timers();
    registry::clear();
}

/// Placement 的读数：走了核间中断几次 / 不必叫的几次。
pub(crate) fn kick_stats() -> (usize, usize) {
    (
        KICKS.load(Ordering::Relaxed),
        FALLBACK.load(Ordering::Relaxed),
    )
}

pub(crate) fn run() -> usize {
    crate::metal::stop::hush();
    crate::basis::sign::endpoint::delivery::advance();
    if !isolated(hart::hart_id()) {
        crate::basis::unit::lifecycle::bury();
    }
    let commit = crate::basis::commit();
    // The timer front half may have retained due tickets when try_commit was
    // busy. Once this scheduler obtains COMMIT, advance them before returning
    // it to competing Yield loops; repeated front-half rejection must not
    // starve a deadline whose owner is already making scheduling progress.
    sign::redeem();
    if let Some(route) = super::interrupt::external() {
        route.retry(&commit);
    }
    if slot::current()
        .running_task()
        .is_some_and(|task| crate::basis::unit::lifecycle::take_doomed(task.ident.id).is_some())
    {
        drop(commit);
        return crate::basis::unit::lifecycle::quit();
    }
    let next = slot::current().advance(&commit);
    drop(commit);
    match next {
        Some(pa) => pa,
        None => idle::fetch(),
    }
}

fn dispatch(allow_steal: bool) -> Option<usize> {
    // Ready tasks must not escape every scheduler container between commits.
    let _commit = crate::basis::commit();
    let redeemed = sign::redeem();
    if let Some(route) = super::interrupt::external() {
        route.retry(&_commit);
    }
    let s = slot::current();
    s.awaken();
    s.relocate();
    let task = s.dequeue(&_commit).or_else(|| {
        if allow_steal && !redeemed {
            steal(&_commit)
        } else {
            None
        }
    });
    task.and_then(|task| s.dispatch(task, &_commit))
}
