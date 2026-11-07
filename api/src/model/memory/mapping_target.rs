use crate::model::identity::{PendId, TeamId};
/// A CPU space selected by Team control, or an I/O space selected by a trusted
/// MMIO View's Pend. Numeric addresses never mint an external routing authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MappingTarget {
    Cpu(TeamId),
    Io(PendId),
}
