#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum MappingState {
    Publishing = 1,
    Active = 2,
    Retiring = 3,
    Settled = 4,
    Failed = 5,
}
/// Retained I/O projection state. A reported fault never proves reclamation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MappingInfo {
    pub state: MappingState,
    pub fault_cause: usize,
    pub fault_address: usize,
}
