//! 物理承载实例。身份不取自 PA、Flat 槽号或引用地址，复用物理页会产生新身份。
//!
//! 接入分配器管理的层级连续 RAM。外部物理区间仍使用 BorrowedRegion；
//! 托管 Page 在分配时归属于 Kernel 或 Team；裸物理别名不创建新的物理身份。

mod identity;
mod region;
mod shape;
use region::Region;
pub(crate) use region::Repartition;
mod owner;
pub(crate) use owner::Owner;
pub(super) mod reclaim;

use alloc::alloc::Allocator;
use alloc::boxed::Box;
use alloc::sync::Arc;
use core::cell::UnsafeCell;
use core::{alloc::Layout, ptr::NonNull};
use kernel_api::model::cell::CellLevel;

use kernel_api::model::cell::PAGE_SIZE;
use kernel_api::model::cell::PageOwner;

pub(crate) use identity::PageId;
pub(crate) use reclaim::{Block, Change, Freeze, Maintenance, RetainError, Retention};

/// 分配器交出的唯一承载。构造 Page 后只有 Page 保留这份回收凭证。
pub(crate) type InternalPage = Box<UnsafeCell<[u8; PAGE_SIZE]>, &'static (dyn Allocator + Sync)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PageError {
    InvalidRange,
    InvalidOwner,
    IdentityExhausted,
    OutOfMemory,
    Busy,
}

/// Page 的外部访问风险；Internal 不表示普通持有或 CPU 访问已经结束。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Exposure {
    Internal,
    External,
}

/// 一个托管 RAM Page；自身不可 Clone，建立槽后由 Arc<Page> 保留同一实例。
///
/// Page 析构是该承载唯一的物理归还出口，还须通过自身硬件责任账目的判定。
/// 页槽保留实例；Nest 在发布前登记硬件责任，在失效成功后显式结清。
/// 丢失已发布责任会永久保留承载，不能把普通引用消失解释成硬件已静止。
/// owner 是唯一资源归属；Flat 槽、映射和借用仅持有同一 Page。
/// External 从已登记的设备访问责任推导，不证明独占借用。
/// 尚未登记的裸物理访问不在本账目保护范围内。
pub(crate) struct Page {
    id: PageId,
    owner: Owner,
    page: Option<Region>,
    level: CellLevel,
    reclaim: reclaim::Reclaim,
    membership: crate::metal::sync::SpinLock<reclaim::group::Membership>,
}

impl Page {
    /// 消费已分配的 RAM 页；失败也由传入的唯一凭证归还，调用方不再手动释放。
    /// 分配/清零在 Flat 分配器一侧进行，这里只建立身份与回收责任。
    pub(crate) fn from_internal(owner: Owner, page: InternalPage) -> Result<Self, PageError> {
        if matches!(owner.identity(), PageOwner::Team(team) if team.get() == 0) {
            return Err(PageError::InvalidOwner);
        }
        let start = page.get().addr();
        if !start.is_multiple_of(PAGE_SIZE) || start.checked_add(PAGE_SIZE - 1).is_none() {
            return Err(PageError::InvalidRange);
        }
        let (pointer, allocator) = Box::into_raw_with_allocator(page);
        Self::from_region(
            owner,
            Region {
                pointer: NonNull::new(pointer.cast()).expect("allocated page"),
                layout: Layout::new::<UnsafeCell<[u8; PAGE_SIZE]>>(),
                allocator,
                repartition: None,
            },
            CellLevel::BASE,
        )
    }

    fn from_region(owner: Owner, region: Region, level: CellLevel) -> Result<Self, PageError> {
        Ok(Self {
            id: PageId::allocate().ok_or(PageError::IdentityExhausted)?,
            owner,
            page: Some(region),
            level,
            reclaim: reclaim::Reclaim::new(),
            membership: crate::metal::sync::SpinLock::new(reclaim::group::Membership::new()),
        })
    }

    /// # Safety
    /// `repartition` must implement the supplied allocator's receipt contract.
    pub(crate) unsafe fn allocate(
        owner: Owner,
        level: CellLevel,
        allocator: &'static (dyn Allocator + Sync),
        repartition: Repartition,
    ) -> Result<Self, PageError> {
        if matches!(owner.identity(), PageOwner::Team(team) if team.get() == 0) {
            return Err(PageError::InvalidOwner);
        }
        if level == CellLevel::BASE {
            let page: InternalPage = unsafe {
                Box::try_new_zeroed_in(allocator)
                    .map_err(|_| PageError::OutOfMemory)?
                    .assume_init()
            };
            let mut result = Self::from_internal(owner, page)?;
            result.page.as_mut().unwrap().repartition = Some(repartition);
            return Ok(result);
        }
        let size = level.bytes().ok_or(PageError::InvalidRange)?;
        let layout = Layout::from_size_align(size, size).map_err(|_| PageError::InvalidRange)?;
        let pointer = allocator
            .allocate_zeroed(layout)
            .map_err(|_| PageError::OutOfMemory)?
            .cast();
        Self::from_region(
            owner,
            Region {
                pointer,
                layout,
                allocator,
                repartition: Some(repartition),
            },
            level,
        )
    }

    pub(crate) fn level(&self) -> CellLevel {
        self.level
    }
    pub(crate) fn size(&self) -> usize {
        self.level.bytes().unwrap()
    }
    pub(crate) fn pages(&self) -> usize {
        self.level.pages().unwrap()
    }

    pub(crate) fn begin(self: &Arc<Self>) -> Result<Arc<Change>, RetainError> {
        Change::begin(self)
    }

    pub(crate) fn id(&self) -> PageId {
        self.id
    }

    pub(crate) fn owner(&self) -> PageOwner {
        self.owner.identity()
    }

    pub(crate) fn physical_address(&self) -> usize {
        self.as_mut_ptr().addr()
    }

    pub(crate) fn as_mut_ptr(&self) -> *mut u8 {
        self.page
            .as_ref()
            .expect("physical receipt")
            .pointer
            .as_ptr()
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
        Retention::prepare(self, reclaim::Kind::External, modifiable)
    }

    /// Advisory preflight for precise staged errors. Actual registration repeats
    /// the group check under the same membership gate as Retention admission.
    pub(crate) fn compatible(
        &self,
        topology: &crate::metal::generic::memory::topology::Topology,
        endpoint: crate::metal::generic::memory::Endpoint,
        access: crate::metal::generic::memory::Access,
    ) -> Result<(), crate::metal::generic::memory::Error> {
        self.membership
            .lock()
            .check(self, topology, endpoint, access)
    }
    pub(crate) fn frozen(&self) -> bool {
        self.reclaim.frozen()
    }

    pub(crate) fn changing(&self) -> bool {
        self.reclaim.is_changing()
    }

    pub(crate) fn exposure(&self) -> Exposure {
        self.reclaim.exposure()
    }

    /// 只回答硬件责任是否允许物理归还；实际归还还要求持有已全部结束。
    pub(crate) fn allows_return(&self) -> bool {
        self.reclaim.allows_return()
    }
    pub(crate) fn obligations(&self) -> u64 {
        self.reclaim.obligations()
    }
    pub(crate) fn maintenance_obligations(&self) -> u64 {
        self.reclaim.maintenance()
    }
    pub(crate) fn quarantined(&self) -> bool {
        self.reclaim.quarantined()
    }
}

impl Drop for Page {
    fn drop(&mut self) {
        if self.allows_return() {
            // SAFETY: Page exclusively owns the allocation and no registered
            // hardware obligation remains. This is its only physical return path.
            drop(self.page.take());
        }
        // 无法证明安全时不执行 allocator deallocate，宁可保留物理容量。
        if let Some(region) = self.page.take() {
            core::mem::forget(region);
        }
    }
}

// SAFETY: Page never creates Rust references to the payload. Consumers of its raw
// pointer must supply their own access/synchronization proof; the owned allocation
// remains alive while a unique Page or any Arc<Page> retention exists.
// InternalPage requires a Sync allocator so reclamation may occur on any hart.
unsafe impl Send for Page {}
unsafe impl Sync for Page {}

pub(crate) use reclaim::statistics;

impl core::fmt::Debug for Page {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Page")
            .field("id", &self.id)
            .field("level", &self.level)
            .field("reclaim", &self.reclaim)
            .finish_non_exhaustive()
    }
}
