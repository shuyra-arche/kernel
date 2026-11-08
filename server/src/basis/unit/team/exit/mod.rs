mod backend;
mod mode;
mod policy;
mod state;

pub(crate) use policy::ExitPolicy;

use kernel_api::abi::call::unit::UnitFail;
use kernel_api::model::identity::TaskId;

use crate::basis::unit::task::Task;

use super::life::TeamLife;

pub(crate) fn member_dying(task: &Task, reason: usize) {
    let mut node = Some(&*task.ident.team.life);
    while let Some(team) = node {
        if team.exit.trip() {
            backend::stop(reason);
        }
        node = team.parent.as_deref();
    }
}

pub(crate) fn arm(team: &TeamLife, owner: TaskId) -> Result<(), UnitFail> {
    team.exit.arm(owner)
}
