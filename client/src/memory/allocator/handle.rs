//! Stateless opt-in registration handle for the one program heap.

pub struct HeapAllocator;

#[cfg(target_arch = "riscv64")]
unsafe impl core::alloc::GlobalAlloc for HeapAllocator {
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
        unsafe { core::alloc::GlobalAlloc::alloc(&super::HEAP, layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: core::alloc::Layout) {
        unsafe { core::alloc::GlobalAlloc::dealloc(&super::HEAP, pointer, layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: core::alloc::Layout) -> *mut u8 {
        unsafe { core::alloc::GlobalAlloc::alloc_zeroed(&super::HEAP, layout) }
    }
    unsafe fn realloc(
        &self,
        pointer: *mut u8,
        layout: core::alloc::Layout,
        size: usize,
    ) -> *mut u8 {
        unsafe { core::alloc::GlobalAlloc::realloc(&super::HEAP, pointer, layout, size) }
    }
}
