//! Scoped authority, immutable derivation and Task-local registrations.

pub(crate) mod access;
mod authority;
mod branch;
mod ledger;
mod source;
pub(crate) use authority::{Export, Pend, PreparedLimit};
pub(crate) use source::Tracking;
pub(crate) use source::maintenance::{MaintenanceUse, Stage};
mod target;
mod window;
pub(crate) use target::Target;

pub(crate) use kernel_api::model::modifiable::Modifiable;

use crate::basis::unit::task::Task;

pub(crate) use branch::{accord, reduce};
pub(crate) use ledger::{
    PendEntry, PreparedReservationGrant, Table, accede, available, doom, insert, locate, notify,
    prepare_reservation, retire, return_, revoke, vestor,
};
pub(crate) use source::{
    ExternalBorrow, Geometry, Hold, HoldTable, RangeBlock, SourceAuthorization,
};
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
