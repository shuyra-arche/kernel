use crate::cell::{Owner, Page, PageError};
use kernel_api::model::cell::CellLevel;
use std::{
    alloc::{AllocError, Allocator, Layout},
    ptr::NonNull,
    sync::{Arc, Mutex},
};
const SIZE: usize = 1 << 21;
struct Pool {
    base: usize,
    used: Mutex<Vec<bool>>,
}
impl Pool {
    fn new() -> &'static Self {
        let base = unsafe { std::alloc::alloc(Layout::from_size_align(SIZE, SIZE).unwrap()) };
        assert!(!base.is_null());
        Box::leak(Box::new(Self {
            base: base as usize,
            used: Mutex::new(vec![false; 512]),
        }))
    }
    fn used(&self) -> usize {
        self.used.lock().unwrap().iter().filter(|x| **x).count()
    }
    fn page(&'static self) -> Arc<Page> {
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
#[test]
fn split_merge_preserve_bytes_and_return_each_receipt_once() {
    let pool = Pool::new();
    let page = pool.page();
    assert_eq!(page.level().get(), 1);
    let id = page.id();
    unsafe {
        page.as_mut_ptr().write(7);
        page.as_mut_ptr().add(SIZE - 1).write(9);
    }
    let mut original = vec![page];
    let mut children = Page::reshape(&mut original, CellLevel::BASE).unwrap();
    assert!(original.is_empty());
    assert_eq!(children.len(), 512);
    assert_ne!(children[0].id(), id);
    let merged = Page::reshape(&mut children, CellLevel::new(1).unwrap()).unwrap();
    assert!(children.is_empty());
    assert_ne!(merged[0].id(), id);
    unsafe {
        assert_eq!(merged[0].as_mut_ptr().read(), 7);
        assert_eq!(merged[0].as_mut_ptr().add(SIZE - 1).read(), 9);
    }
    assert_eq!(pool.used(), 512);
    drop(merged);
    assert_eq!(pool.used(), 0);
    let reused = pool.page();
    unsafe {
        assert_eq!(reused.as_mut_ptr().read(), 0);
        assert_eq!(reused.as_mut_ptr().add(SIZE - 1).read(), 0);
    }
    drop(reused);
    assert_eq!(pool.used(), 0);
}
#[test]
fn reshape_rejects_aliases_and_unsettled_external_access() {
    let pool = Pool::new();
    let page = pool.page();
    let alias = page.clone();
    let mut pages = vec![page];
    assert_eq!(
        Page::reshape(&mut pages, CellLevel::BASE).unwrap_err(),
        PageError::Busy
    );
    drop(alias);
    let mut external = pages[0].prepare_external(true).unwrap();
    external.publish();
    assert_eq!(
        Page::reshape(&mut pages, CellLevel::BASE).unwrap_err(),
        PageError::Busy
    );
    unsafe {
        external.retire().settle();
    }
    drop(Page::reshape(&mut pages, CellLevel::BASE).unwrap());
    assert_eq!(pool.used(), 0);
}
#[test]
fn incomplete_or_reordered_children_preserve_receipts() {
    let pool = Pool::new();
    let mut pages = vec![pool.page()];
    let mut children = Page::reshape(&mut pages, CellLevel::BASE).unwrap();
    let last = children.pop().unwrap();
    let id = children[0].id();
    assert_eq!(
        Page::reshape(&mut children, CellLevel::new(1).unwrap()).unwrap_err(),
        PageError::InvalidRange
    );
    assert_eq!(children[0].id(), id);
    children.push(last);
    children.swap(0, 1);
    assert_eq!(
        Page::reshape(&mut children, CellLevel::new(1).unwrap()).unwrap_err(),
        PageError::InvalidRange
    );
    assert_eq!(pool.used(), 512);
    drop(children);
    assert_eq!(pool.used(), 0);
}
#[test]
fn preparation_failure_keeps_original_identity_and_capacity() {
    let pool = Pool::new();
    let mut pages = vec![pool.page()];
    let id = pages[0].id();
    unsafe {
        pages[0].as_mut_ptr().add(4096).write(53);
    }
    for count in [0, 1, 2, 257, 512] {
        let result =
            crate::allocation::fail_after(count, || Page::reshape(&mut pages, CellLevel::BASE));
        assert_eq!(result.unwrap_err(), PageError::OutOfMemory);
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].id(), id);
        assert_eq!(pool.used(), 512);
        unsafe {
            assert_eq!(pages[0].as_mut_ptr().add(4096).read(), 53);
        }
    }
    drop(Page::reshape(&mut pages, CellLevel::BASE).unwrap());
    assert_eq!(pool.used(), 0);
}
