pub(crate) use crate::external_error::Error as PublicationError;
#[path = "../../../src/basis/nest/error.rs"]
mod error;
pub(crate) use error::NestError;
pub(crate) mod address {
    pub(crate) enum AddressError {
        OutOfMemory,
        NotAligned,
        NoRegion,
        Occupied,
    }
}
pub(crate) mod authority {
    pub(crate) struct RangeBinding;
}
mod memo;
mod translation;

// Inert address owners for the production CPU Retirement contract. The tests
// exercise real retained Publications and request matching, not an MMU or index.
type Asid = usize;
struct Span {
    id: usize,
    state: crate::host_lock::TestLock<SpanState>,
}
struct SpanState {
    source: Option<()>,
    active: Vec<ActiveRange>,
    memos: memo::Memos,
}
impl SpanState {
    fn take_deleted(&mut self, base: kernel_api::address::VirtAddr) -> Option<memo::Memo> {
        self.memos.take_deleted(base, &self.active)
    }
}
struct ActiveRange;
impl ActiveRange {
    fn contains(&self, _: usize) -> bool {
        true
    }
}
impl Span {
    fn new(id: usize) -> Self {
        Self {
            id,
            state: crate::host_lock::TestLock::new(SpanState {
                source: None,
                active: Vec::new(),
                memos: memo::Memos::new(),
            }),
        }
    }
    fn identity(&self) -> usize {
        self.id
    }
    fn initial_start(&self) -> kernel_api::address::VirtAddr {
        kernel_api::address::VirtAddr::new(0)
    }
    fn changed(&self) {}
}
