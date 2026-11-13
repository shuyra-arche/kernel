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
struct Span;
impl Span {
    fn changed(&self) {}
}
struct Nest {
    state: crate::host_lock::SpinLock<RetirementState>,
}
struct RetirementState {
    memos: memo::Memos,
}
impl Nest {
    fn finish_memo(&self, _: &alloc::sync::Arc<Span>, _: usize) {
        panic!("logical completion outside CPU retirement fixture")
    }
}
pub(crate) mod index {
    pub(crate) mod source {
        pub(crate) fn wake_all() {}
    }
}
