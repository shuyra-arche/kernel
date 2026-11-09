use crate::metal::allocator::InitResult;
use alloc::boxed::Box;
use core::ptr::NonNull;
use erra::ResultExt;

use alloc::alloc::AllocError;
use alloc::alloc::Allocator;

use crate::metal::allocator::InitError;
use kernel_api::model::cell::PAGE_SIZE;

mod inner;

use inner::FrameInner;

pub struct FrameAllocator {
    inner: crate::metal::sync::SpinLock<FrameInner>,
}

impl FrameAllocator {
    pub fn new() -> Result<Self, InitError> {
        let mut inner = FrameInner::new();
        inner.init()?;
        Ok(Self {
            inner: crate::metal::sync::SpinLock::new_level(crate::metal::sync::Level::Frame, inner),
        })
    }
}

fn block_power(size: usize) -> usize {
    size.max(PAGE_SIZE)
        .next_multiple_of(PAGE_SIZE)
        .next_power_of_two()
        .ilog2() as usize
        - PAGE_SIZE.ilog2() as usize
}

unsafe impl Allocator for FrameAllocator {
    fn allocate(&self, layout: core::alloc::Layout) -> Result<NonNull<[u8]>, AllocError> {
        crate::metal::allocator::assert_allocation_allowed();
        self.inner.with(|frame| {
            let size = layout.size().max(PAGE_SIZE);
            let power = block_power(size.max(layout.align()));
            let index = unsafe { frame.split_block(power) }.ok_or(AllocError)?;
            let addr = frame.frame_addr(index) as *mut u8;
            #[cfg(debug_assertions)]
            {
                let a = addr as usize;
                assert!(
                    (a >= frame.base) && (a < frame.edge),
                    "frame alloc out of range: {a:#x} not in [{:#x}, {:#x})",
                    frame.base,
                    frame.edge
                );
            }
            crate::metal::allocator::statistics::record_frame_take(index, power);
            Ok(NonNull::slice_from_raw_parts(
                NonNull::new(addr).ok_or(AllocError)?,
                size,
            ))
        })
    }

    fn allocate_zeroed(&self, layout: core::alloc::Layout) -> Result<NonNull<[u8]>, AllocError> {
        let block = self.allocate(layout)?;
        // The allocator lock is already released. Large Cells keep receiving
        // interrupts without exposing partially initialized memory to a Task.
        for offset in (0..block.len()).step_by(64 * 1024) {
            let count = (block.len() - offset).min(64 * 1024);
            unsafe {
                block
                    .as_ptr()
                    .cast::<u8>()
                    .add(offset)
                    .write_bytes(0, count)
            };
            crate::metal::interrupt::window::poll();
        }
        Ok(block)
    }

    #[track_caller]
    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: core::alloc::Layout) {
        unsafe {
            self.inner.with(|frame| {
                let size = layout.size().max(PAGE_SIZE);
                let power = block_power(size.max(layout.align()));
                let addr = ptr.addr().get();
                #[cfg(debug_assertions)]
                {
                    let a = addr;
                    assert!(
                        (a >= frame.base) && (a < frame.edge),
                        "frame dealloc out of range: {a:#x} not in [{:#x}, {:#x})",
                        frame.base,
                        frame.edge
                    );
                }
                let index = frame.frame_index(addr);
                crate::metal::allocator::statistics::record_frame_give(index);
                frame.merge_block(index, power);
            })
        }
    }
}

static FRAME_ALLOCATOR: crate::metal::sync::OnceLock<&'static FrameAllocator> =
    crate::metal::sync::OnceLock::new();

pub(crate) fn instance() -> &'static FrameAllocator {
    FRAME_ALLOCATOR
        .get()
        .expect("frame allocator not initialized")
}

pub fn allocator() -> &'static dyn Allocator {
    instance()
}

pub fn init() -> InitResult<()> {
    (|| -> Result<(), InitError> {
        let mut slot =
            Box::<FrameAllocator>::try_new_uninit().map_err(|_| InitError::OutOfMemory)?;
        slot.write(FrameAllocator::new()?);
        let heap = Box::leak(unsafe { slot.assume_init() });
        FRAME_ALLOCATOR
            .set(heap)
            .map_err(|_| InitError::AlreadyInitialized)
    })()
    .annotate("initializing frame allocator")
}

/// Repartition exclusive receipts without changing the allocator's free blocks.
/// # Safety
/// Old and new aligned receipts must cover the same live, contiguous allocation union.
pub(crate) unsafe fn repartition(
    base: usize,
    old: kernel_api::model::cell::CellLevel,
    old_count: usize,
    new: kernel_api::model::cell::CellLevel,
    new_count: usize,
) {
    instance().inner.with(|frame| {
        for index in 0..old_count {
            crate::metal::allocator::statistics::record_frame_give(
                frame.frame_index(base + index * old.bytes().unwrap()),
            );
        }
        for index in 0..new_count {
            crate::metal::allocator::statistics::record_frame_take(
                frame.frame_index(base + index * new.bytes().unwrap()),
                new.get() * 9,
            );
        }
    });
}
