//! Unique allocator receipt, including its supported repartition operation.
use alloc::alloc::Allocator;
use core::{alloc::Layout, ptr::NonNull};
use kernel_api::model::cell::CellLevel;
/// # Safety
/// The allocator accepts independently returned aligned subdivisions and coalesced
/// adjacent allocations. The callback updates accounting without freeing or unwinding.
pub(crate) type Repartition = unsafe fn(usize, CellLevel, usize, CellLevel, usize);
pub(super) struct Region {
    pub(super) pointer: NonNull<u8>,
    pub(super) layout: Layout,
    pub(super) allocator: &'static (dyn Allocator + Sync),
    pub(super) repartition: Option<Repartition>,
}
impl Region {
    pub(super) fn compatible(&self, other: &Self) -> bool {
        core::ptr::eq(self.allocator, other.allocator)
            && self
                .repartition
                .zip(other.repartition)
                .is_some_and(|(a, b)| core::ptr::fn_addr_eq(a, b))
    }
}
impl core::fmt::Debug for Region {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Region")
            .field("pointer", &self.pointer)
            .field("layout", &self.layout)
            .finish()
    }
}
impl Drop for Region {
    fn drop(&mut self) {
        unsafe { self.allocator.deallocate(self.pointer, self.layout) }
    }
}
