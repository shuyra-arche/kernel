//! Cell geometric allocation fixture, also consumed by Flat reshaping tests.
use crate::cell::{Owner, Page};
use kernel_api::model::cell::CellLevel;
use std::{
    alloc::{AllocError, Allocator, Layout},
    ptr::NonNull,
    sync::{Arc, Mutex},
};

pub(super) const SIZE: usize = 1 << 21;
pub(super) struct Pool {
    base: usize,
    used: Mutex<Vec<bool>>,
}
impl Pool {
    pub(super) fn new() -> &'static Self {
        let base = unsafe { std::alloc::alloc(Layout::from_size_align(SIZE, SIZE).unwrap()) };
        assert!(!base.is_null());
        Box::leak(Box::new(Self {
            base: base as usize,
            used: Mutex::new(vec![false; 512]),
        }))
    }
    pub(super) fn used(&self) -> usize {
        self.used.lock().unwrap().iter().filter(|x| **x).count()
    }
    pub(super) fn page(&'static self) -> Arc<Page> {
        Arc::new(
            unsafe { Page::allocate(Owner::Kernel, CellLevel::new(1).unwrap(), self, repartition) }
                .unwrap(),
        )
    }
}
unsafe fn repartition(_: usize, _: CellLevel, _: usize, _: CellLevel, _: usize) {}
unsafe impl Allocator for Pool {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        let count = layout.size().max(4096).div_ceil(4096);
        let mut used = self.used.lock().unwrap();
        for start in 0..=512usize.checked_sub(count).ok_or(AllocError)? {
            let address = self.base + start * 4096;
            if address.is_multiple_of(layout.align())
                && used[start..start + count].iter().all(|x| !*x)
            {
                used[start..start + count].fill(true);
                return Ok(NonNull::slice_from_raw_parts(
                    NonNull::new(address as *mut u8).unwrap(),
                    count * 4096,
                ));
            }
        }
        Err(AllocError)
    }
    unsafe fn deallocate(&self, pointer: NonNull<u8>, layout: Layout) {
        let start = (pointer.as_ptr() as usize - self.base) / 4096;
        let count = layout.size().max(4096).div_ceil(4096);
        let mut used = self.used.lock().unwrap();
        assert!(used[start..start + count].iter().all(|x| *x));
        used[start..start + count].fill(false);
    }
}
