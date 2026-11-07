use std::alloc::AllocError;
use std::alloc::Allocator;
use std::alloc::Layout;
use std::cell::UnsafeCell;
use std::ptr::NonNull;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use kernel_api::model::cell::PAGE_SIZE;

use super::cell::InternalPage;

#[repr(align(4096))]
struct Storage(UnsafeCell<[u8; 2 * PAGE_SIZE]>);

/// A single reusable allocation, with observable take/give effects.
pub struct Pool {
    storage: Storage,
    offset: usize,
    busy: AtomicBool,
    pub gives: AtomicUsize,
}

impl Pool {
    pub fn new(offset: usize) -> &'static Self {
        assert!(offset <= PAGE_SIZE);
        Box::leak(Box::new(Self {
            storage: Storage(UnsafeCell::new([0xA5; 2 * PAGE_SIZE])),
            offset,
            busy: AtomicBool::new(false),
            gives: AtomicUsize::new(0),
        }))
    }

    pub fn page(&'static self) -> Result<InternalPage, AllocError> {
        let allocator: &'static (dyn Allocator + Sync) = self;
        // SAFETY: all-zero bytes are valid for UnsafeCell<[u8; PAGE_SIZE]>.
        unsafe { Box::try_new_zeroed_in(allocator).map(|page| page.assume_init()) }
    }

    pub fn gives(&self) -> usize {
        self.gives.load(Ordering::SeqCst)
    }

    fn pointer(&self) -> *mut u8 {
        self.storage.0.get().cast::<u8>().wrapping_add(self.offset)
    }
}

// SAFETY: successful allocate authorities unique use until deallocate; the bytes are
// accessed only through that allocation. Counters and admission are atomic.
unsafe impl Sync for Pool {}

unsafe impl Allocator for Pool {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        if layout.size() > PAGE_SIZE
            || !self.pointer().addr().is_multiple_of(layout.align())
            || self
                .busy
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return Err(AllocError);
        }
        Ok(NonNull::slice_from_raw_parts(
            NonNull::new(self.pointer()).unwrap(),
            layout.size(),
        ))
    }

    unsafe fn deallocate(&self, pointer: NonNull<u8>, _layout: Layout) {
        assert_eq!(pointer.as_ptr(), self.pointer());
        assert!(self.busy.swap(false, Ordering::Release));
        self.gives.fetch_add(1, Ordering::SeqCst);
    }
}
