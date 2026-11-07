mod backend;
mod mode;
mod policy;
mod state;

pub(crate) use policy::ExitPolicy;

use kernel_api::{
    abi::call::unit::UnitFail,
    model::identity::TaskId,
    model::unit::{ControlId, ControlKind},
};

use crate::core::work::unit::task::Task;

use super::life::TeamLife;

pub(crate) fn conflicts(life: &TeamLife) -> bool {
    let mut node = life.parent.as_deref();
    while let Some(team) = node {
        if team.exit.protected() {
            return true;
        }
        node = team.parent.as_deref();
    }
    let mut descendant = false;
    life.visit(&mut |team| {
        if team.id != life.id && team.exit.protected() {
            descendant = true;
        }
    });
    descendant
}

pub(crate) fn authorize(caller: &Task, token: ControlId) -> bool {
    caller.administration.matches(token, ControlKind::Guard)
}

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

pub(crate) fn disarm(team: &TeamLife, owner: TaskId) -> Result<(), UnitFail> {
    team.exit.disarm(owner)
}
