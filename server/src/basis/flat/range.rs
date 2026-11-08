use alloc::sync::Arc;
use alloc::vec::Vec;

use kernel_api::address::PageCount;
use kernel_api::address::PageIndex;
use kernel_api::address::PageOffset;
use kernel_api::address::PhysAddr;

use super::Flat;
use super::FlatError;
use crate::basis::cell::page::Page;

/// 强持同一个 `Flat` 的校验范围。子范围用相对偏移选择，合成后仍是原来源身份，
/// 不建立父范围链、不复制数据。
#[derive(Clone)]
pub struct FlatRange {
    flat: Arc<Flat>,
    index: PageIndex,
    count: PageCount,
}

impl FlatRange {
    pub(super) fn new(flat: Arc<Flat>, index: PageIndex, count: PageCount) -> Self {
        Self { flat, index, count }
    }

    pub fn index(&self) -> PageIndex {
        self.index
    }

    /// 这一段窗口强持的那个来源。
    pub(super) fn flat(&self) -> &Arc<Flat> {
        &self.flat
    }

    /// 这一段窗口的来源身份：跨地址空间按来源找关系时做键。
    pub(crate) fn source_id(&self) -> usize {
        Arc::as_ptr(&self.flat) as usize
    }

    pub fn page_count(&self) -> PageCount {
        self.count
    }

    /// 是不是**同一份来源上的同一段窗口**（Flat 身份与坐标都一致）。
    ///
    /// 用来判断一段关系能不能代表整份来源——子范围/切片都不能。
    pub(crate) fn same_range(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.flat, &other.flat)
            && self.index == other.index
            && self.count == other.count
    }

    /// 相对偏移选择子范围。
    pub fn range(&self, offset: PageOffset, count: PageCount) -> Result<Self, FlatError> {
        let end = offset
            .get()
            .checked_add(count.get())
            .ok_or(FlatError::OutOfRange)?;
        if end > self.count.get() {
            return Err(FlatError::OutOfRange);
        }
        let index = PageIndex::new(
            self.index()
                .get()
                .checked_add(offset.get())
                .ok_or(FlatError::OutOfRange)?,
        );
        self.flat.range(index, count)
    }

    /// 取得完整、稳定的物理持有：Managed 必要时物化，Borrowed 校验并保留借用区间。
    pub fn materialize(&self) -> Result<Pages, FlatError> {
        let residency = self.reside()?;
        self.flat.materialize_range(self.index, self.count)?;
        Ok(Pages::new(residency))
    }

    pub fn reside(&self) -> Result<Residency, FlatError> {
        self.flat.reside(self.clone(), None)
    }

    pub(crate) fn reside_during(
        &self,
        change: &crate::basis::cell::page::Change,
    ) -> Result<Residency, FlatError> {
        self.flat.reside(self.clone(), Some(change))
    }

    pub(super) fn release_residency(&self, id: usize) {
        self.flat.release_residency(id);
    }

    /// 起点之内第 `offset` 页此刻能不能物化出承载：`Fresh`/`Resident` 可以，
    /// `Missing` 与在途页不行；越界是独立错误。
    ///
    /// 条件等待的只读口：不能拿“物理出口成不成功”代替它——那会把 `Fresh` 与
    /// 承载移走的墓碑混同，于是回退回 `Fresh` 的页面永远等不到。
    pub(crate) fn can_materialize(&self, offset: PageOffset) -> Result<bool, FlatError> {
        self.flat.can_materialize(self.page_at(offset)?)
    }

    /// 这份来源永久失去迁移资格（整份来源，不是这一段窗口）。
    pub(crate) fn mark_untransferable(&self) {
        self.flat.mark_untransferable();
    }

    /// 只读稀疏已物化的 `Page`；不物化、不保证整个范围完整；借用来源没有 `Page`。
    /// 偏移相对范围起点。
    pub(crate) fn materialized_cells(&self) -> Result<Vec<(PageOffset, Arc<Page>)>, FlatError> {
        self.flat.materialized(self.index, self.count)
    }

    pub(crate) fn is_borrowed(&self) -> bool {
        self.flat.is_borrowed()
    }

    /// 起点之内第 `offset` 页的来源绝对页号；越界即拒。
    pub(crate) fn page_at(&self, offset: PageOffset) -> Result<PageIndex, FlatError> {
        if offset.get() >= self.count.get() {
            return Err(FlatError::OutOfRange);
        }
        let page = self
            .index
            .get()
            .checked_add(offset.get())
            .ok_or(FlatError::OutOfRange)?;
        Ok(PageIndex::new(page))
    }

    /// 起点之内第 `offset` 页的物理地址；页槽自己的状态按自己的失败报。
    ///
    /// 物理地址是地址信息，不是持有凭证。
    pub(super) fn physical(&self, offset: PageOffset) -> Result<PhysAddr, FlatError> {
        self.flat.physical_at(self.page_at(offset)?)
    }
}

/// Blocks ownership changes for this logical window until the last actual holder drops it.
pub struct Residency {
    range: FlatRange,
    id: usize,
}

impl Residency {
    pub(super) fn new(range: FlatRange, id: usize) -> Self {
        Self { range, id }
    }

    pub fn range(&self) -> &FlatRange {
        &self.range
    }
}

impl Drop for Residency {
    fn drop(&mut self) {
        self.range.release_residency(self.id);
    }
}

/// 一个来源范围的完整、稳定物化结果。
///
/// Managed 的页此时已全部物化，Borrowed 的借用凭证仍被引用持有；它只由
/// `FlatRange::materialize` 构造，不用裸 `Vec<PhysAddr>` 代替持有。
pub struct Pages {
    residency: Residency,
}

impl Pages {
    pub(crate) fn new(residency: Residency) -> Self {
        Self { residency }
    }

    /// 范围起点之内第 `offset` 页的物理地址；物理地址是地址信息，不是持有凭证。
    pub(crate) fn physical(&self, offset: PageOffset) -> Result<PhysAddr, FlatError> {
        self.residency.range().physical(offset)
    }
}
