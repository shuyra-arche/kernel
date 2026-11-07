//! 逻辑承载组织：RAM 页槽持有 Cell，外部范围暂保留借用凭证。
//!
//! Flat 不含 VA、页表、Task 或授权账目；它决定槽的物化与迁移，
//! 槽持有不建立另一份物理所有权。共享访问使用同一 Flat 的范围与映射。

pub mod allocator;

mod allocation;
mod error;
mod pages;
mod range;
mod region;
mod residency;
mod residency_entry;
mod slot;
mod source;
mod state;
pub(in crate::core::memory) mod transaction;

use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::core::memory::PAGE_SIZE;
use crate::core::memory::address::{PageCount, PageIndex, PageOffset, PhysAddr};
use crate::core::memory::cell::Cell;
use crate::machine::sync::{Level, SpinLock};

pub use allocation::Allocation;
pub(crate) use allocator::frame::cell as allocate_cell;
pub use error::FlatError;
pub use pages::Pages;
pub use range::FlatRange;
pub use region::BorrowedRegion;
pub use residency::Residency;

use residency_entry::ResidencyEntry;
use slot::Slot;
use source::FlatSource;
use state::FlatState;

/// 线性逻辑页来源。
///
/// 要么是分配器 RAM（页槽按需物化），要么是调用方仍保留的外部物理区间。
/// 物化时机由调用方组合决定，来源本身不携带 eager / lazy 策略。
pub struct Flat {
    page_count: PageCount,
    state: SpinLock<FlatState>,
}

impl Flat {
    /// 建立 RAM 逻辑来源；不提前物化。这种来源是可迁移的。
    pub fn acquire(allocation: Allocation) -> Result<Arc<Self>, FlatError> {
        Self::new(
            allocation.page_count(),
            FlatSource::Managed(Vec::new()),
            true,
        )
    }

    /// 消费新 Cell 建立逻辑槽；借来的 Arc<Cell> 不能导入为第二个承载来源。
    ///
    /// 输入必须按页号严格递增；失败释放这批尚未发布的 Cell。
    /// 导入来源保持不可迁移，沿用启动/镜像/任务内存的既有约束。
    pub(crate) fn from_cells(
        page_count: PageCount,
        cells: Vec<(PageIndex, Cell)>,
    ) -> Result<Arc<Self>, FlatError> {
        let slots = slot::admit(page_count.get(), cells)?;
        Self::new(page_count, FlatSource::Managed(slots), false)
    }

    /// 保留外部借用物理区间：不分配数据页、不清零、不按 RAM 回收；也不可迁移。
    pub fn from_region(region: Arc<BorrowedRegion>) -> Result<Arc<Self>, FlatError> {
        let page_count = PageCount::new(region.size() / PAGE_SIZE).ok_or(FlatError::OutOfRange)?;
        Self::new(page_count, FlatSource::Borrowed(region), false)
    }

    fn new(
        page_count: PageCount,
        source: FlatSource,
        transferable: bool,
    ) -> Result<Arc<Self>, FlatError> {
        Arc::try_new(Self {
            page_count,
            state: SpinLock::new_level(Level::Flat, FlatState::new(source, transferable)),
        })
        .map_err(|_| FlatError::OutOfMemory)
    }

    /// 这份来源永久失去迁移资格：已经有一处内核或全局映射压在它上面。
    ///
    /// 不可恢复，而且与读取它的那次资格检查共用同一把状态锁：
    /// “检查过资格”与“之后才发布全局映射”不会交错成半个结论。
    pub(super) fn mark_untransferable(&self) {
        self.state.lock().transferable = false;
    }

    /// 来源声明的逻辑页总数。
    pub fn page_count(&self) -> PageCount {
        self.page_count
    }

    /// 整段来源。
    pub fn whole(self: &Arc<Self>) -> FlatRange {
        FlatRange::new(self.clone(), PageIndex::ZERO, self.page_count())
    }

    /// 按来源位置选择范围。
    pub fn range(
        self: &Arc<Self>,
        index: PageIndex,
        count: PageCount,
    ) -> Result<FlatRange, FlatError> {
        let end = index
            .get()
            .checked_add(count.get())
            .ok_or(FlatError::OutOfRange)?;
        if end > self.page_count.get() {
            return Err(FlatError::OutOfRange);
        }
        Ok(FlatRange::new(self.clone(), index, count))
    }

    pub(super) fn is_borrowed(&self) -> bool {
        matches!(&self.state.lock().source, FlatSource::Borrowed(_))
    }

    pub(super) fn reside(&self, range: FlatRange) -> Result<Residency, FlatError> {
        let mut state = self.state.lock();
        // 驻留要在在途事务之外取得：Moving/Reserved 的页还没有可用的承载。
        if state.busy(range.index(), range.page_count()) {
            return Err(FlatError::Busy);
        }
        let next = state
            .next_residency
            .checked_add(1)
            .ok_or(FlatError::OutOfMemory)?;
        state
            .residencies
            .try_reserve(1)
            .map_err(|_| FlatError::OutOfMemory)?;
        let id = state.next_residency;
        state.residencies.push(ResidencyEntry {
            id,
            index: range.index(),
            count: range.page_count(),
        });
        state.next_residency = next;
        Ok(Residency::new(range, id))
    }

    pub(super) fn release_residency(&self, id: usize) {
        let mut state = self.state.lock();
        let at = state
            .residencies
            .iter()
            .position(|entry| entry.id == id)
            .expect("residency belongs to its source");
        state.residencies.remove(at);
    }

    /// 物化 [index, index + count)：只有 Fresh 的页会被分配，已有承载的页跳过。
    ///
    /// 已是 `Missing` 的页不补零也不新分配——那是承载移走后的墓碑，
    /// 只有移交能把新承载交进它。中途失败保留已经成功的页槽；调用方仍可重试。
    pub(super) fn materialize_range(
        &self,
        index: PageIndex,
        count: PageCount,
    ) -> Result<(), FlatError> {
        let end = index
            .get()
            .checked_add(count.get())
            .ok_or(FlatError::OutOfRange)?;
        if end > self.page_count.get() {
            return Err(FlatError::OutOfRange);
        }
        let mut guard = self.state.lock();
        let FlatSource::Managed(slots) = &mut guard.source else {
            return Ok(());
        };
        let mut page = index.get();
        let mut at = slots.partition_point(|(slot, _)| slot.get() < page);
        while page < end {
            if slots.get(at).is_some_and(|(slot, _)| slot.get() == page) {
                match slots[at].1 {
                    // 承载已移出：不补零、不新分配。
                    Slot::Missing => return Err(FlatError::Missing),
                    // 正被一次移交持有/预留：何时能用不由物化说了算。
                    Slot::Moving { .. } | Slot::Reserved { .. } => return Err(FlatError::Busy),
                    Slot::Resident(_) => {}
                }
                page += 1;
                at += 1;
                continue;
            }
            slots.try_reserve(1).map_err(|_| FlatError::OutOfMemory)?;
            let cell = Arc::try_new(allocate_cell()?).map_err(|_| FlatError::OutOfMemory)?;
            slots.insert(at, (PageIndex::new(page), Slot::Resident(cell)));
            page += 1;
            at += 1;
        }
        Ok(())
    }

    /// 窗口内已物化页槽的稀疏快照，偏移相对窗口起点。
    ///
    /// 只有 `Resident` 是空闲的承载：`Missing` 是墓碑，`Moving`/`Reserved`
    /// 属于在途事务，都不进快照。
    pub(super) fn materialized(
        &self,
        index: PageIndex,
        count: PageCount,
    ) -> Result<Vec<(PageOffset, Arc<Cell>)>, FlatError> {
        let end = index
            .get()
            .checked_add(count.get())
            .ok_or(FlatError::OutOfRange)?;
        if end > self.page_count.get() {
            return Err(FlatError::OutOfRange);
        }
        let guard = self.state.lock();
        let FlatSource::Managed(slots) = &guard.source else {
            return Ok(Vec::new());
        };
        let first = slots.partition_point(|(slot, _)| slot.get() < index.get());
        let last = slots.partition_point(|(slot, _)| slot.get() < end);
        let mut out = Vec::new();
        out.try_reserve(last - first)
            .map_err(|_| FlatError::OutOfMemory)?;
        for (slot, state) in &slots[first..last] {
            if let Slot::Resident(cell) = state {
                out.push((PageOffset::new(slot.get() - index.get()), cell.clone()));
            }
        }
        Ok(out)
    }

    /// 这一页此刻能不能物化出承载：`Fresh` 与 `Resident` 可以；`Missing` 墓碑与
    /// 在途的 `Moving`/`Reserved` 不行。借用来源的合法页正常可用；越界是独立错误。
    ///
    /// 只读短判据：不分配、不发布、不产生失效，也不新造页状态或世代。
    pub(super) fn can_materialize(&self, page: PageIndex) -> Result<bool, FlatError> {
        if page.get() >= self.page_count.get() {
            return Err(FlatError::OutOfRange);
        }
        let guard = self.state.lock();
        match &guard.source {
            FlatSource::Borrowed(_) => Ok(true),
            FlatSource::Managed(slots) => {
                let (at, present) = slot::spot(slots, page);
                if !present {
                    // 缺席就是 Fresh：它本就有一份可以分配的承载。
                    return Ok(true);
                }
                Ok(matches!(slots[at].1, Slot::Resident(_)))
            }
        }
    }

    pub(super) fn physical_at(&self, page: PageIndex) -> Result<PhysAddr, FlatError> {
        if page.get() >= self.page_count.get() {
            return Err(FlatError::OutOfRange);
        }
        let guard = self.state.lock();
        match &guard.source {
            FlatSource::Borrowed(region) => {
                let offset = page
                    .get()
                    .checked_mul(PAGE_SIZE)
                    .ok_or(FlatError::OutOfRange)?;
                let address = region
                    .start()
                    .as_usize()
                    .checked_add(offset)
                    .ok_or(FlatError::OutOfRange)?;
                Ok(PhysAddr::from_raw(address))
            }
            FlatSource::Managed(slots) => {
                let (at, present) = slot::spot(slots, page);
                if !present {
                    return Err(FlatError::OutOfRange);
                }
                match &slots[at].1 {
                    Slot::Resident(cell) => Ok(PhysAddr::from_raw(cell.physical_address())),
                    // 承载已移出：这是页槽自己的状态，不抹成“越界”。
                    Slot::Missing => Err(FlatError::Missing),
                    // 在途事务持有/预留的页没有可交出去的物理坐标。
                    Slot::Moving { .. } | Slot::Reserved { .. } => Err(FlatError::Busy),
                }
            }
        }
    }
}
