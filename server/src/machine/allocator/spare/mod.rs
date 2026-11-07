use crate::machine::allocator::InitResult;
use crate::machine::sync::OnceLock;
use alloc::boxed::Box;
use core::alloc::AllocError;
use core::alloc::Allocator;
use core::alloc::Layout;
use core::ptr::NonNull;
use erra::ResultExt;

use crate::machine::allocator::InitError;
use kernel_api::model::cell::PAGE_SIZE;

mod blk;

mod inner;

use blk::Blk;

use inner::SpareInner;

const HEADER: usize = 32;
const MAX_ALIGN: usize = 16;

pub const DUMP_BUDGET: usize = 1024 * 1024;

pub struct SpareAllocator {
    inner: crate::machine::sync::SpinLock<SpareInner>,
}

impl SpareAllocator {
    pub fn new() -> Result<Self, InitError> {
        let cap = (HEADER + DUMP_BUDGET).next_multiple_of(PAGE_SIZE);
        let align = Layout::from_size_align(cap, PAGE_SIZE).map_err(|_| InitError::OutOfMemory)?;
        let chunk = crate::tag!(
            Spare,
            crate::machine::allocator::hybrid::allocator().allocate(align)
        )
        .map_err(|_| InitError::OutOfMemory)?;
        let base = chunk.as_ptr() as *mut u8 as usize;
        let edge = base + chunk.len();
        Ok(Self {
            inner: crate::machine::sync::SpinLock::new_level(
                crate::machine::sync::Level::Spare,
                SpareInner::new(base, edge),
            ),
        })
    }

    pub fn total_bytes(&self) -> usize {
        self.inner.with(|g| g.edge - g.base)
    }
}

unsafe impl Allocator for SpareAllocator {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        crate::machine::allocator::assert_allocation_allowed();
        if layout.align() > MAX_ALIGN {
            return Err(AllocError);
        }
        let need = layout.size().max(1).next_multiple_of(MAX_ALIGN);
        self.inner.with(|inner| {
            let Some(blk) = inner.pull(need) else {
                return Err(AllocError);
            };
            let addr = blk.as_ptr() as usize;
            let size = unsafe { blk.read() }.size;
            crate::machine::allocator::statistics::record_spare_take(size);
            Ok(NonNull::slice_from_raw_parts(
                NonNull::new((addr + HEADER) as *mut u8).ok_or(AllocError)?,
                layout.size(),
            ))
        })
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, _layout: Layout) {
        unsafe {
            let blk_addr = ptr.as_ptr() as usize - HEADER;
            self.inner.with(|inner| {
                if !inner.owns(blk_addr) {
                    return;
                }

                let blk = NonNull::new_unchecked(blk_addr as *mut Blk);
                let size = blk.read().size;
                crate::machine::allocator::statistics::record_spare_give(size);
                inner.push(blk);
            })
        }
    }
}

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
        let heap =
            Box::leak(Box::try_new(SpareAllocator::new()?).map_err(|_| InitError::OutOfMemory)?);
        SPARE_ALLOCATOR
            .set(heap)
            .map_err(|_| InitError::AlreadyInitialized)
    })()
    .annotate("initializing spare allocator")
}
