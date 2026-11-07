use core::alloc::{AllocError, Allocator, Layout};
use core::ptr::NonNull;

use alloc::boxed::Box;
use erra::ResultExt;

use crate::basis::memory::flat::allocator::{InitError, InitResult, hybrid, statistics};
use crate::machine::sync::{OnceLock, SpinLock};
use kernel_allocator::spare::{Effect, SpareAllocator as Mechanism};

pub(crate) struct KernelSpare;

// SAFETY: 仓区从同一实际 Hybrid 实例取得并在 Spare
// 生存期内独占；统计/禁令直接转发 Kernel 当前函数。
unsafe impl Effect for KernelSpare {
    fn allocate_spare(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        crate::tag!(Spare, hybrid::allocator().allocate(layout))
    }

    fn record_spare_take(&self, bytes: usize) {
        statistics::record_spare_take(bytes);
    }

    fn record_spare_give(&self, bytes: usize) {
        statistics::record_spare_give(bytes);
    }

    fn assert_allocation_allowed(&self) {
        crate::basis::memory::flat::allocator::assert_allocation_allowed();
    }
}

pub(crate) type SpareAllocator = Mechanism<SpinLock<()>, KernelSpare>;

static SPARE_ALLOCATOR: OnceLock<&'static SpareAllocator> = OnceLock::new();

pub fn spare() -> &'static SpareAllocator {
    SPARE_ALLOCATOR
        .get()
        .expect("spare allocator not initialized")
}

pub fn allocator() -> &'static dyn Allocator {
    spare()
}

pub fn init() -> InitResult<()> {
    (|| -> Result<(), InitError> {
        let heap = Box::leak(
            Box::try_new(SpareAllocator::new(KernelSpare)?).map_err(|_| InitError::OutOfMemory)?,
        );
        SPARE_ALLOCATOR
            .set(heap)
            .map_err(|_| InitError::AlreadyInitialized)
    })()
    .annotate("initializing spare allocator")
}
