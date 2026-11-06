use core::ptr::NonNull;

use alloc::alloc::{AllocError, Allocator};

use crate::{InitError, Lock, LockRole, Locked};
use kernel_api::model::memory::PAGE_SIZE;

mod effect;
#[path = "inner.rs"]
mod inner;

pub use effect::Effect;
use inner::FrameInner;

pub struct FrameAllocator<L: Lock, E: Effect> {
    effect: E,
    inner: Locked<L, FrameInner>,
}

impl<L: Lock, E: Effect> FrameAllocator<L, E> {
    pub fn new(effect: E) -> Result<Self, InitError> {
        let mut inner = FrameInner::new();
        inner.init(&effect)?;
        Ok(Self {
            effect,
            inner: Locked::new(L::new(Some(LockRole::Frame)), inner),
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

unsafe impl<L: Lock, E: Effect> Allocator for FrameAllocator<L, E> {
    fn allocate(&self, layout: core::alloc::Layout) -> Result<NonNull<[u8]>, AllocError> {
        self.effect.assert_allocation_allowed();
        self.inner.with(|frame| {
            let size = layout.size().max(PAGE_SIZE);
            let power = block_power(size);
            let index = unsafe { frame.split_block(&self.effect, power) }.ok_or(AllocError)?;
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
            self.effect.record_frame_take(index, power);
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
                self.effect.record_frame_give(index);
                frame.merge_block(&self.effect, index, power);
            })
        }
    }
}
