use crate::metal::allocator::InitResult;
use crate::metal::allocator::frame;
use core::alloc::AllocError;
use core::alloc::Allocator;
use core::alloc::Layout;
use core::ptr::NonNull;

use crate::metal::allocator::block;

pub struct HybridAllocator {}

impl HybridAllocator {
    pub const fn new() -> Self {
        Self {}
    }
}

unsafe impl Allocator for HybridAllocator {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        crate::metal::allocator::assert_allocation_allowed();
        if layout.size() <= block::MAX_SIZE {
            block::heap().allocate(layout)
        } else {
            crate::metal::allocator::frame::instance().allocate(layout)
        }
    }

    #[track_caller]
    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        unsafe {
            if layout.size() <= block::MAX_SIZE {
                block::heap().deallocate(ptr, layout);
            } else {
                crate::metal::allocator::frame::instance().deallocate(ptr, layout);
            }
        }
    }
}

pub(crate) static HYBRID_ALLOCATOR: HybridAllocator = HybridAllocator::new();

pub fn allocator() -> &'static dyn Allocator {
    &HYBRID_ALLOCATOR
}

pub fn init() -> InitResult<()> {
    block::init()?;
    frame::init()?;
    Ok(())
}
