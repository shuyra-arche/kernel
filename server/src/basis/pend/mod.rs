//! Authority over View ranges and the records that bind it to a Task.

pub(crate) mod access;
mod authority;
mod branch;
mod ledger;
mod source;
pub(crate) use authority::Pend;
mod target;
mod window;
pub(crate) use target::Target;

pub(crate) use kernel_api::model::modifiable::Modifiable;

use crate::basis::unit::task::Task;

pub(crate) use branch::{accord, reduce};
pub(crate) use ledger::{
    PendEntry, Table, accede, available, doom, insert, locate, notify, retire, return_, revoke,
    vestor,
};
pub(crate) use source::{ExternalBorrow, Geometry, Hold, HoldTable, SourceAuthorization};
pub(crate) use window::Window;

pub(crate) fn live(task: &Task) -> bool {
    !task.dead() && !task.ident.team.node.closed()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Need {
    Read,
    Write,
}
impl Need {
    pub(crate) fn allowed_by(self, access: Modifiable) -> bool {
        match self {
            Self::Write => access == Modifiable::Yes,
            Self::Read => true,
        }
    }
}
