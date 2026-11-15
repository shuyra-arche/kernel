use crate::basis::cell::page::Owner;
use crate::cell::InternalPage;
use crate::cell::Page;
use alloc::boxed::Box;
use core::alloc::AllocError;
use core::alloc::Allocator;
use core::alloc::GlobalAlloc;
use core::alloc::Layout;
use core::cell::UnsafeCell;
use core::ptr::NonNull;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

#[repr(align(4096))]
struct Storage<const N: usize>(UnsafeCell<[u8; N]>);
unsafe impl<const N: usize> Sync for Storage<N> {}

static HEAP: Storage<65536> = Storage(UnsafeCell::new([0; 65536]));
static FRONT: AtomicUsize = AtomicUsize::new(0);
struct Bump;
#[global_allocator]
static ALLOCATOR: Bump = Bump;

unsafe impl GlobalAlloc for Bump {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut front = FRONT.load(Ordering::Relaxed);
        loop {
            let start = front.next_multiple_of(layout.align());
            let Some(end) = start.checked_add(layout.size()).filter(|end| *end <= 65536) else {
                return core::ptr::null_mut();
            };
            match FRONT.compare_exchange_weak(front, end, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => return unsafe { HEAP.0.get().cast::<u8>().add(start) },
                Err(actual) => front = actual,
            }
        }
    }
    unsafe fn dealloc(&self, _: *mut u8, _: Layout) {}
}

static PAGES: Storage<32768> = Storage(UnsafeCell::new([0; 32768]));
pub static LIMIT: AtomicUsize = AtomicUsize::new(3);
static USED: AtomicUsize = AtomicUsize::new(0);
pub static RETURNS: AtomicUsize = AtomicUsize::new(0);
pub struct Pool;
pub static POOL: Pool = Pool;

impl Pool {
    pub fn cell(&'static self) -> Option<Page> {
        let allocator: &'static (dyn Allocator + Sync) = self;
        let page: InternalPage = unsafe { Box::try_new_zeroed_in(allocator).ok()?.assume_init() };
        Page::from_internal(Owner::Kernel, page).ok()
    }
}

unsafe impl Allocator for Pool {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        if layout.size() > 4096 || layout.align() > 4096 {
            return Err(AllocError);
        }
        let mut used = USED.load(Ordering::Relaxed);
        loop {
            let limit = LIMIT.load(Ordering::Relaxed);
            let slot = (!used & ((1 << limit) - 1)).trailing_zeros() as usize;
            if slot >= limit {
                return Err(AllocError);
            }
            match USED.compare_exchange_weak(
                used,
                used | (1 << slot),
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    return Ok(NonNull::slice_from_raw_parts(
                        unsafe {
                            NonNull::new_unchecked(PAGES.0.get().cast::<u8>().add(slot * 4096))
                        },
                        layout.size(),
                    ));
                }
                Err(actual) => used = actual,
            }
        }
    }
    unsafe fn deallocate(&self, pointer: NonNull<u8>, _: Layout) {
        let offset = pointer.as_ptr().addr() - PAGES.0.get().addr();
        assert!(offset < 32768 && offset.is_multiple_of(4096));
        let mask = 1 << (offset / 4096);
        assert_ne!(USED.fetch_and(!mask, Ordering::AcqRel) & mask, 0);
        RETURNS.fetch_add(1, Ordering::SeqCst);
    }
}
