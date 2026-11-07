//! 物理承载实例。身份不取自 PA、Flat 槽号或引用地址，复用物理页会产生新身份。
//!
//! 本批只接入分配器管理的 4 KiB RAM。外部物理区间仍使用 BorrowedRegion；
//! 在统一外部登记与 Unit 归属协议建立前，不把裸物理映射伪装成新的 Cell。

mod identity;

use alloc::alloc::Allocator;
use alloc::boxed::Box;
use core::cell::UnsafeCell;

use kernel_api::model::memory::PAGE_SIZE;

pub(crate) use identity::CellId;

/// 分配器交出的唯一承载。构造 Cell 后只有 Cell 保留这份回收凭证。
pub(crate) type InternalPage = Box<UnsafeCell<[u8; PAGE_SIZE]>, &'static (dyn Allocator + Sync)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CellError {
    InvalidRange,
    IdentityExhausted,
}

/// 一个 Internal Cell；自身不可 Clone，建立槽后由 Arc<Cell> 保留同一实例。
///
/// `page` 的析构是该承载唯一的物理归还出口。页槽、映射与退休记录
/// 只负责持有或解除持有。保留记录的完备性仍由现有 Nest 失效协议保证，
/// 不能把引用计数本身解释成硬件已静止或独占借用的证明。
#[derive(Debug)]
pub(crate) struct Cell {
    id: CellId,
    page: InternalPage,
}

impl Cell {
    /// 消费已分配的 RAM 页；失败也由传入的唯一凭证归还，调用方不再手动释放。
    /// 分配/清零在 Flat 分配器一侧进行，这里只建立身份与回收责任。
    pub(crate) fn from_internal(page: InternalPage) -> Result<Self, CellError> {
        let start = page.get().addr();
        if !start.is_multiple_of(PAGE_SIZE) || start.checked_add(PAGE_SIZE - 1).is_none() {
            return Err(CellError::InvalidRange);
        }
        Ok(Self {
            id: CellId::allocate().ok_or(CellError::IdentityExhausted)?,
            page,
        })
    }

    pub(crate) fn id(&self) -> CellId {
        self.id
    }

    pub(crate) fn physical_address(&self) -> usize {
        self.page.get().addr()
    }

    pub(crate) fn as_mut_ptr(&self) -> *mut u8 {
        self.page.get().cast()
    }
}

// SAFETY: Cell never creates Rust references to the payload. Consumers of its raw
// pointer must supply their own access/synchronization proof; the owned allocation
// remains alive while a unique Cell or any Arc<Cell> retention exists.
// InternalPage requires a Sync allocator so reclamation may occur on any hart.
unsafe impl Send for Cell {}
unsafe impl Sync for Cell {}
