mod facts;

use facts::MachineFacts;

use crate::machine::allocator::InitResult;
use alloc::boxed::Box;
use core::ptr::NonNull;
use erra::ResultExt;

use alloc::alloc::AllocError;
use alloc::alloc::Allocator;

use crate::machine::allocator::InitError;
use kernel_api::model::cell::PAGE_SIZE;

mod inner;

use inner::FrameInner;

pub struct FrameAllocator {
    inner: crate::machine::sync::SpinLock<FrameInner>,
}

impl FrameAllocator {
    pub fn new() -> Result<Self, InitError> {
        let mut inner = FrameInner::new();
        inner.init()?;
        Ok(Self {
            inner: crate::machine::sync::SpinLock::new_level(
                crate::machine::sync::Level::Frame,
                inner,
            ),
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
        crate::machine::allocator::assert_allocation_allowed();
        self.inner.with(|frame| {
            let size = layout.size().max(PAGE_SIZE);
            let power = block_power(size);
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
            crate::machine::allocator::statistics::record_frame_take(index, power);
            Ok(NonNull::slice_from_raw_parts(
                NonNull::new(addr).ok_or(AllocError)?,
                size,
            ))
        })
    }

    #[track_caller]
    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: core::alloc::Layout) {
        unsafe {
            self.inner.with(|frame| {
                let size = layout.size().max(PAGE_SIZE);
                let power = block_power(size);
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
                crate::machine::allocator::statistics::record_frame_give(index);
                frame.merge_block(index, power);
            })
        }
    }
}

fn machine_facts() -> MachineFacts {
    let m = crate::machine::board::info();
    MachineFacts {
        reserved: m
            .reserved
            .map(|r| r.map(|r| super::Region::new(r.base, r.size))),
    }
}

static FRAME_ALLOCATOR: crate::machine::sync::OnceLock<&'static FrameAllocator> =
    crate::machine::sync::OnceLock::new();

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
