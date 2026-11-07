use super::{MappingControl, Placement, ReferenceRange};
/// A single immutable attach request snapshot. A nonzero change receipt is
/// supported for I/O publication; CPU Memo supply uses Push.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attachment {
    pub range: ReferenceRange,
    pub placement: Placement,
    pub access: u64,
    pub control: MappingControl,
    pub change: usize,
}
