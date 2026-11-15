use kernel_api::address::VirtAddr;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u64)]
pub(crate) enum MemoState {
    Vacant = 0,
    Ready = 1,
    Published = 2,
    Publishing = 3,
    Retracting = 4,
    Failed = 5,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MemoInfo {
    pub level: kernel_api::model::cell::CellLevel,
    pub generation: u64,
    pub state: MemoState,
    pub page: u64,
    pub frozen: bool,
    pub external: bool,
    pub address: VirtAddr,
    pub fault_cause: u64,
    pub fault_address: u64,
}
