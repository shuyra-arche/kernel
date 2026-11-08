use super::{
    Allocation, MappingControl, MappingId, MappingTarget, Placement, ReferenceRange, SlotId,
};
use crate::{address::VirtAddr, model::identity::PendId};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellObject {
    Slot(SlotId),
    Memo {
        target: MappingTarget,
        mapping: MappingId,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellSource {
    Slot(ReferenceRange),
    Pend {
        token: PendId,
        offset: usize,
        count: usize,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellCreate {
    Slot(Allocation),
    /// Reserve stable positions: Slot source starts Vacant; Pend source supplies Ready associations.
    /// Neither form publishes hardware access.
    Memo {
        target: MappingTarget,
        source: CellSource,
        placement: Placement,
        access: u64,
        control: MappingControl,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CreatedCell {
    pub kind: u64,
    pub id: u64,
    pub address: VirtAddr,
}
/// Generation qualifies a Memo transition. Slot endpoints use generation zero;
/// they move the current unique holding and do not claim a content snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellPosition {
    pub object: CellObject,
    pub page: usize,
    pub generation: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellHandoff {
    pub source: CellPosition,
    pub target: CellPosition,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u64)]
pub enum CellState {
    Vacant = 0,
    Ready = 1,
    Published = 2,
    Publishing = 3,
    Retracting = 4,
    Failed = 5,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellInfo {
    pub generation: u64,
    pub state: CellState,
    pub page: u64,
    pub frozen: bool,
    pub external: bool,
    pub address: VirtAddr,
    pub fault_cause: u64,
    pub fault_address: u64,
}
