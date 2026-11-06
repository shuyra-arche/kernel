//! Task creation and domain-local closure execution.

pub mod task;
pub(crate) mod tls;

use crate::call as env;

use env::{TaskId, TeamId, UnitResult, VirtAddr};

/// Create a held task with scalar startup arguments.
pub fn spawn(team: TeamId, entry: usize, args: &[usize], stack: usize) -> UnitResult<TaskId> {
    env::unit::spawn(
        team,
        entry,
        VirtAddr::new(args.as_ptr() as usize),
        args.len(),
        stack,
    )
}

pub use env::unit::{debark, embark, self_id, slay};

/// Observe native completion without consuming a receipt or changing local TLS.
pub fn observe(target: env::UnitTarget, wait: env::Wait) -> UnitResult<env::JoinReply> {
    env::unit::join(target, wait, false)
}

pub fn join_task(task: TaskId, wait: env::Wait) -> UnitResult<bool> {
    observe(task.into(), wait).map(|reply| reply.is_reaped())
}
pub fn embark_task(task: TaskId) -> UnitResult<()> {
    embark(task.into())
}
pub fn debark_task(task: TaskId) -> UnitResult<()> {
    debark(task.into())
}
pub fn slay_task(task: TaskId) -> UnitResult<()> {
    slay(task.into())
}
pub fn embark_team(team: TeamId) -> UnitResult<()> {
    embark(team.into())
}
pub fn debark_team(team: TeamId) -> UnitResult<()> {
    debark(team.into())
}
pub fn slay_team(team: TeamId) -> UnitResult<()> {
    slay(team.into())
}
