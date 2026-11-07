use crate::machine::allocator::InitResult;
use alloc::alloc::AllocError;
use alloc::alloc::Allocator;
use core::ptr::NonNull;
use erra::ResultExt;

use crate::machine::allocator::InitError;
use crate::machine::allocator::Region;

mod inner;

use inner::BumpInner;

pub struct BumpAllocator {
    inner: crate::machine::sync::SpinLock<Option<BumpInner>>,
}

impl BumpAllocator {
    pub const fn new() -> Self {
        Self {
            inner: crate::machine::sync::SpinLock::new(None),
        }
    }

    /// 以调用方实际拥有的独占区域建立 bump 前沿。
    ///
    /// # Safety
    ///
    /// `free` 必须在整个分配器（及其派生 Frame）生存期内有效、独占、页对齐，且
    /// `free.base + free.size` 不溢出；不得与任何独立分配器的区域重叠；unsafe 重新初始化
    /// 不得使仍存活的既有分配与新区间重叠（不得重发重叠地址）。
    pub unsafe fn init(&self, free: Region) -> Result<(), InitError> {
        self.inner.with(|slot| {
            let mut inner = BumpInner::new(0, 0, 0);
            inner.init(free)?;
            *slot = Some(inner);
            Ok(())
        })
    }

    pub fn boundary(&self) -> usize {
        self.inner
            .with(|slot| slot.as_ref().expect("bump allocator not initialized").edge)
    }

    pub fn frontier(&self) -> usize {
        self.inner.with(|slot| {
            let inner = slot.as_ref().expect("bump allocator not initialized");
            inner.base + inner.used
        })
    }
}

unsafe impl Allocator for BumpAllocator {
    fn allocate(
        &self,
        layout: core::alloc::Layout,
    ) -> Result<core::ptr::NonNull<[u8]>, AllocError> {
        self.inner.with(|slot| {
            let inner = slot.as_mut().ok_or(AllocError)?;

            let frontier = (inner.base + inner.used) as *mut u8;
            let next = unsafe { frontier.add(frontier.align_offset(layout.align())) };

            if next.addr() + layout.size() > inner.edge {
                return Err(AllocError);
            }

            inner.used = next.addr() - inner.base + layout.size();
            Ok(NonNull::slice_from_raw_parts(
                NonNull::new(next).ok_or(AllocError)?,
                layout.size(),
            ))
        })
    }

    unsafe fn deallocate(&self, _ptr: core::ptr::NonNull<u8>, _layout: core::alloc::Layout) {}
}

pub(crate) static BUMP_ALLOCATOR: BumpAllocator = BumpAllocator::new();

pub fn allocator() -> &'static dyn Allocator {
    &BUMP_ALLOCATOR
}

pub(crate) fn boundary() -> usize {
    BUMP_ALLOCATOR.boundary()
}

pub(crate) fn frontier() -> usize {
    BUMP_ALLOCATOR.frontier()
}

pub fn init() -> InitResult<()> {
    let free = crate::machine::board::info().free;
    // SAFETY: board::info().free 是内核实际拥有的 free DRAM 区间；它在整个分配器
    // 生存期内独占有效，Bump 与随后派生的 Frame 都只从这一实例取前沿。
    unsafe { BUMP_ALLOCATOR.init(Region::new(free.base, free.size)) }
        .annotate("initializing bump allocator")
}
