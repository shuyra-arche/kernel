//! Per-Hart admission and bounded service-dependency coordination. Task owns
//! budget state; this index retains only weak registrations. COMMIT serializes
//! admission, graph decisions and scheduling publication.
use crate::basis::unit::task::{
    Task,
    schedule::{
        Schedule,
        account::{Account, Contract, MAX_TASKS, analyze, priorities},
    },
};
use crate::metal::{
    clock,
    hart::{self, HartId},
};
use alloc::sync::{Arc, Weak};
use core::sync::atomic::Ordering;
use kernel_api::{
    abi::call::{sign::SignFail, unit::UnitFail},
    model::unit::{Schedule as Specification, ScheduleInfo},
};

pub(super) struct Admission {
    tasks: [Option<Weak<Task>>; MAX_TASKS],
}
impl Admission {
    pub(super) const fn new() -> Self {
        Self {
            tasks: [const { None }; MAX_TASKS],
        }
    }
    fn members(&mut self) -> [Option<Arc<Task>>; MAX_TASKS] {
        core::array::from_fn(|at| {
            let task = self.tasks[at].as_ref().and_then(Weak::upgrade);
            if task
                .as_ref()
                .is_none_or(|task| task.dead() || !task.realtime())
            {
                self.tasks[at] = None;
                None
            } else {
                task
            }
        })
    }
}
fn ticks(ns: u64) -> u64 {
    clock::duration_to_ticks(core::time::Duration::from_nanos(ns))
}
fn nanos(ticks: u64) -> u64 {
    clock::ticks_to_duration(ticks)
        .as_nanos()
        .min(u64::MAX as u128) as u64
}

pub(crate) fn admit(
    actor: &Arc<Task>,
    task: &Arc<Task>,
    spec: Specification,
) -> Result<(), UnitFail> {
    if Specification::from_bytes(&spec.to_bytes()).is_none()
        || spec.hart as usize >= hart::hart_count()
    {
        return Err(UnitFail::Denied);
    }
    let destination = HartId::new(spec.hart as usize);
    // An ordinary Task may prepare its grants before its controller freezes it.
    // Frozen Ready has been detached from its scheduler and retained by Team.
    let _commit = crate::basis::commit();
    if !authorized(actor, task)
        || !(task.held() || task.stopped() && task.ready() && task.on_hart().is_none())
        || !matches!(*task.schedule.lock(), Schedule::Ordinary)
        || task.dead()
        || !task.heir.lock().is_empty()
        || task.ident.team.nest.kind().is_supervisor()
        || Task::execution_capability(destination) != Task::execution_capability(HartId::new(0))
    {
        return Err(UnitFail::Denied);
    }
    *task.schedule.lock() = Schedule::Preparing;
    let preparation = Preparation(task.clone());
    drop(_commit);
    task.prepare_extensions(destination, spec.extensions)?;
    task.ident.prepare_resident().map_err(|_| UnitFail::Busy)?;
    let _commit = crate::basis::commit();
    if !authorized(actor, task)
        || task.dead()
        || !matches!(*task.schedule.lock(), Schedule::Preparing)
    {
        return Err(UnitFail::Denied);
    }
    let slot = &super::schedulers()[destination.get()];
    let mut admission = slot.admission.lock();
    let members = admission.members();
    let at = members
        .iter()
        .position(Option::is_none)
        .ok_or(UnitFail::OutOfMemory)?;
    let mut contracts = [Contract {
        spec,
        budget: 1,
        period: 1,
        latency: 0,
        response: 0,
    }; MAX_TASKS];
    let mut count = 0;
    for task in members.iter().flatten() {
        contracts[count] = task.schedule.lock().contract().expect("admitted Task");
        count += 1;
    }
    contracts[count] = Contract {
        spec,
        budget: clock::budget_ticks(spec.budget),
        period: ticks(spec.period),
        latency: ticks(spec.latency),
        response: 0,
    };
    if !analyze(&mut contracts[..=count]) {
        return Err(UnitFail::Busy);
    }
    for (task, contract) in members.iter().flatten().zip(&contracts[..count]) {
        if let Schedule::Fixed {
            contract: current, ..
        } = &mut *task.schedule.lock()
        {
            *current = *contract;
        }
    }
    let contract = contracts[count];
    *task.schedule.lock() = Schedule::Fixed {
        contract,
        account: Account::new(contract.budget, contract.period),
        effective: spec.priority as u8,
        dependency: None,
    };
    admission.tasks[at] = Some(Arc::downgrade(task));
    slot.isolated.store(true, Ordering::Release);
    drop(admission);
    drop(preparation);
    super::idle::nudge(destination);
    Ok(())
}
fn authorized(actor: &Task, task: &Task) -> bool {
    !actor.dead()
        && !actor.ident.team.node.closed()
        && actor
            .administration
            .allows(kernel_api::model::unit::ControlKind::Build)
        && crate::basis::unit::control::manages(actor, task)
}

/// Cancel the preparation lease on failure; a successful publication owns it.
struct Preparation(Arc<Task>);
impl Drop for Preparation {
    fn drop(&mut self) {
        let _commit = crate::basis::commit();
        let mut state = self.0.schedule.lock();
        if matches!(*state, Schedule::Preparing) {
            *state = Schedule::Ordinary;
        }
    }
}
pub(crate) fn evict(actor: &Task, task: &Arc<Task>) -> Result<(), UnitFail> {
    let _commit = crate::basis::commit();
    if !authorized(actor, task) || !task.held() || !task.realtime() {
        return Err(UnitFail::Denied);
    }
    release(task);
    Ok(())
}
pub(crate) fn activated(task: &Task) -> bool {
    let contract = {
        let state = task.schedule.lock();
        if matches!(*state, Schedule::Preparing) {
            return false;
        }
        state.contract()
    };
    contract.is_none_or(|contract| {
        super::schedulers()[contract.spec.hart as usize]
            .running_task()
            .is_none_or(|running| running.realtime())
    })
}
pub(crate) fn release(task: &Task) {
    let _commit = crate::basis::commit();
    let Some(contract) = task.schedule.lock().contract() else {
        return;
    };
    *task.schedule.lock() = Schedule::Ordinary;
    let slot = &super::schedulers()[contract.spec.hart as usize];
    let mut admission = slot.admission.lock();
    let members = admission.members();
    slot.isolated
        .store(members.iter().any(Option::is_some), Ordering::Release);
    drop(admission);
    if let Some(actor) = members.iter().flatten().next() {
        let server = match &*actor.schedule.lock() {
            Schedule::Fixed { dependency, .. } => dependency
                .as_ref()
                .and_then(Weak::upgrade)
                .filter(|task| !task.dead()),
            Schedule::Ordinary | Schedule::Preparing => None,
        };
        let _ = depend(actor, server.as_ref());
    }
    // A server exit also invalidates callers' retained service binding.
    notify();
    super::idle::yell();
}
pub(crate) fn profile(actor: &Task, task: &Task) -> Result<ScheduleInfo, UnitFail> {
    let _commit = crate::basis::commit();
    if actor.dead()
        || actor.ident.team.node.closed()
        || !crate::basis::unit::control::manages(actor, task)
    {
        return Err(UnitFail::Denied);
    }
    let now = clock::now().as_ticks();
    let mut state = task.schedule.lock();
    let Schedule::Fixed {
        contract,
        account,
        effective,
        ..
    } = &mut *state
    else {
        return Err(UnitFail::Denied);
    };
    let remaining = nanos(account.remaining(now));
    Ok(ScheduleInfo {
        schedule: contract.spec,
        effective: *effective as u64,
        remaining,
        replenishment: clock::uptime_at(account.replenishment()),
        consumed: nanos(account.consumed),
        overruns: nanos(account.overruns),
        response: nanos(contract.response),
    })
}

pub(crate) fn depend(actor: &Arc<Task>, server: Option<&Arc<Task>>) -> Result<(), SignFail> {
    let _commit = crate::basis::commit();
    let Some(own) = actor.schedule.lock().contract() else {
        return Err(SignFail::Denied);
    };
    if let Some(server) = server {
        let selected = server.schedule.lock().contract().ok_or(SignFail::Denied)?;
        if server.dead() || actor.ident.id == server.ident.id || own.spec.hart != selected.spec.hart
        {
            return Err(SignFail::Denied);
        }
    }
    let mut admission = super::schedulers()[own.spec.hart as usize].admission.lock();
    let members = admission.members();
    let mut base = [0; MAX_TASKS];
    let mut ceiling = [0; MAX_TASKS];
    let mut edges = [None; MAX_TASKS];
    let mut effective = [0; MAX_TASKS];
    for (at, task) in members.iter().enumerate() {
        let Some(task) = task else { continue };
        let state = task.schedule.lock();
        let Schedule::Fixed {
            contract,
            dependency,
            ..
        } = &*state
        else {
            unreachable!()
        };
        base[at] = contract.spec.priority as u8;
        ceiling[at] = contract.spec.ceiling as u8;
        let target = if task.ident.id == actor.ident.id {
            server.cloned()
        } else {
            dependency.as_ref().and_then(Weak::upgrade)
        };
        if let Some(target) = target {
            edges[at] = members.iter().position(|other| {
                other
                    .as_ref()
                    .is_some_and(|other| Arc::ptr_eq(other, &target))
            });
        }
    }
    if !priorities(&base, &ceiling, &edges, &mut effective) {
        return Err(SignFail::Denied);
    }
    for (at, task) in members.iter().enumerate() {
        let Some(task) = task else { continue };
        if let Schedule::Fixed {
            effective: current,
            dependency,
            ..
        } = &mut *task.schedule.lock()
        {
            *current = effective[at];
            if task.ident.id == actor.ident.id {
                *dependency = server.map(Arc::downgrade);
            }
        }
    }
    drop(admission);
    super::idle::nudge(HartId::new(own.spec.hart as usize));
    Ok(())
}
pub(crate) fn notify() {
    if !super::installed() {
        return;
    }
    for slot in super::schedulers() {
        if slot.isolated.load(Ordering::Acquire) {
            super::idle::nudge(slot.hart);
        }
    }
}
