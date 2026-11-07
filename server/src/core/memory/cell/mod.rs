//! 物理承载实例。身份不取自 PA、Flat 槽号或引用地址，复用物理页会产生新身份。
//!
//! 本批只接入分配器管理的 4 KiB RAM。外部物理区间仍使用 BorrowedRegion；
//! 托管 Cell 在分配时归属于 Kernel 或 Team；裸物理别名不创建新的物理身份。

mod exposure;
mod identity;
mod owner;
pub(crate) use owner::Owner;
mod reclaim;
mod retention;

use alloc::alloc::Allocator;
use alloc::boxed::Box;
use alloc::sync::Arc;
use core::cell::UnsafeCell;
use core::mem::ManuallyDrop;

use kernel_api::model::memory::{CellOwner, PAGE_SIZE};

pub(crate) use exposure::Exposure;
pub(crate) use identity::CellId;
pub(crate) use reclaim::RetainError;
pub(crate) use retention::Retention;

/// 分配器交出的唯一承载。构造 Cell 后只有 Cell 保留这份回收凭证。
pub(crate) type InternalPage = Box<UnsafeCell<[u8; PAGE_SIZE]>, &'static (dyn Allocator + Sync)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CellError {
    InvalidRange,
    InvalidOwner,
    IdentityExhausted,
}

/// 一个托管 RAM Cell；自身不可 Clone，建立槽后由 Arc<Cell> 保留同一实例。
///
/// Cell 析构是该承载唯一的物理归还出口，还须通过自身硬件责任账目的判定。
/// 页槽保留实例；Nest 在发布前登记硬件责任，在失效成功后显式结清。
/// 丢失已发布责任会永久保留承载，不能把普通引用消失解释成硬件已静止。
/// owner 是唯一资源归属；Flat 槽、映射和借用仅持有同一 Cell。
/// External 从已登记的设备访问责任推导，不证明独占借用。
/// 尚未登记的裸物理访问不在本账目保护范围内。
#[derive(Debug)]
pub(crate) struct Cell {
    id: CellId,
    owner: Owner,
    page: ManuallyDrop<InternalPage>,
    reclaim: reclaim::Reclaim,
}

impl Cell {
    /// 消费已分配的 RAM 页；失败也由传入的唯一凭证归还，调用方不再手动释放。
    /// 分配/清零在 Flat 分配器一侧进行，这里只建立身份与回收责任。
    pub(crate) fn from_internal(owner: Owner, page: InternalPage) -> Result<Self, CellError> {
        if matches!(owner.identity(), CellOwner::Team(team) if team.get() == 0) {
            return Err(CellError::InvalidOwner);
        }
        let start = page.get().addr();
        if !start.is_multiple_of(PAGE_SIZE) || start.checked_add(PAGE_SIZE - 1).is_none() {
            return Err(CellError::InvalidRange);
        }
        Ok(Self {
            id: CellId::allocate().ok_or(CellError::IdentityExhausted)?,
            owner,
            page: ManuallyDrop::new(page),
            reclaim: reclaim::Reclaim::new(),
        })
    }

    pub(crate) fn id(&self) -> CellId {
        self.id
    }

    pub(crate) fn owner(&self) -> CellOwner {
        self.owner.identity()
    }

    pub(crate) fn physical_address(&self) -> usize {
        self.page.get().addr()
    }

    pub(crate) fn as_mut_ptr(&self) -> *mut u8 {
        self.page.get().cast()
    }

    /// 先预留共享读或独占写，再由调用方发布；登记失败不能继续建立硬件访问。
    /// CPU 使用可恢复权限的上限预留，临时只读不能绕过未来写访问的互斥。
    pub(crate) fn prepare_hardware(
        self: &Arc<Self>,
        modifiable: bool,
    ) -> Result<Retention, RetainError> {
        Retention::prepare(self, reclaim::Kind::Cpu, modifiable)
    }

    /// 在设备可达之前登记；未发布取消或可信结清后才解除这一条 External 责任。
    pub(crate) fn prepare_external(
        self: &Arc<Self>,
        modifiable: bool,
    ) -> Result<Retention, RetainError> {
        Retention::prepare(self, reclaim::Kind::Device, modifiable)
    }

    pub(crate) fn exposure(&self) -> Exposure {
        self.reclaim.exposure()
    }

    /// 只回答硬件责任是否允许物理归还；实际归还还要求持有已全部结束。
    pub(crate) fn allows_return(&self) -> bool {
        self.reclaim.allows_return()
    }
}

impl Drop for Cell {
    fn drop(&mut self) {
        if self.allows_return() {
            // SAFETY: Cell exclusively owns the allocation and no registered
            // hardware obligation remains. This is its only physical return path.
            unsafe { ManuallyDrop::drop(&mut self.page) };
        }
        // 无法证明安全时不执行 allocator deallocate，宁可保留物理容量。
    }
}

// SAFETY: Cell never creates Rust references to the payload. Consumers of its raw
// pointer must supply their own access/synchronization proof; the owned allocation
// remains alive while a unique Cell or any Arc<Cell> retention exists.
// InternalPage requires a Sync allocator so reclamation may occur on any hart.
unsafe impl Send for Cell {}
unsafe impl Sync for Cell {}
