//! 迁移前的逐页撤销：只撤这一页的 4KiB 映射。
//!
//! 与 `detach` / `close` 的差别在范围与后果：这里不退占用、不改 `MappingId`、
//! 活动形状、VA 与权限，也不把关系摘出索引，更不模拟一次 `detach`。
//! 只把压在这一页上的 PTE 与它的发布记录一起转入退休，等本 Nest 的 ASID
//! 确认之后才放掉承载。

use alloc::sync::Arc;

use crate::basis::memory::PAGE_SIZE;
use crate::basis::memory::address::VirtAddr;
use crate::basis::memory::cell::Cell;

use super::state::NestState;
use super::{Mapping, Nest, NestError};

impl Nest {
    /// 撤下 `mapping` 在 `address` 上的 4KiB 映射，把它那一页的承载交给退休。
    ///
    /// - 内核空间的物理使用不由撤下本关系结清；全局条目不受 ASID 定向失效覆盖。
    ///   两类都不参与移交，当场 `Pinned`。
    /// - 先结清本 Nest 之前那次失效：旧 TLB 没确认就不再撤新的。
    /// - 在 `Nest → Mapping` 同一个临界区里认这一页：只有真有一条发布记录才算这条
    ///   关系握着它；没有记录（活动洞或本来就没发布）就没有可撤的映射，当场
    ///   成功返回。真握着的话，它必须正是 `expected`——不是就在动任何 PTE 之前
    ///   当场断言，不先撤再发现。
    /// - 容量与大叶拆分都在撤下之前备齐，之后不再有可失败步骤。
    /// - 承载只在 shootdown 成功之后随退休一起放掉；失败时这一页的退休责任
    ///   留在本 Nest，由 `retry_invalidation` 继续。
    pub(in crate::basis::memory) fn unpublish_page(
        self: &Arc<Self>,
        mapping: &Arc<Mapping>,
        address: VirtAddr,
        expected: &Arc<Cell>,
    ) -> Result<(), NestError> {
        if self.asid().is_kernel() || mapping.is_global() {
            return Err(NestError::Pinned);
        }
        assert!(
            address.offset() == 0,
            "unpublish_page takes a page-aligned address"
        );
        assert!(
            mapping
                .reservation
                .nest()
                .is_some_and(|owner| core::ptr::eq(Arc::as_ptr(&owner), Arc::as_ptr(self))),
            "unpublish_page requires the mapping's own live nest"
        );
        self.finish_invalidation()?;

        let mut state = self.state.lock();
        if !state.invalidation.is_clean() {
            return Err(NestError::Shootdown);
        }
        let mut inner = mapping.state.lock();
        let page = mapping.page_index(address);
        let Some(at) = inner.published.iter().position(|(held, _)| *held == page) else {
            return Ok(());
        };
        if state.phase.is_closed() {
            return Err(NestError::Pinned);
        }
        // 核对物理 Cell 实例，不能只比较 PA 或逻辑槽；检查先于任何 PTE 撤销。
        assert!(
            inner.published[at].1.id() == expected.id(),
            "the published page holds the Cell this transfer handed over"
        );
        debug_assert!(
            inner.covers(address.as_usize(), address.as_usize()),
            "a published mapping lies inside a live range"
        );
        // 提前备容量与几何：这一页可能只是大叶里的一格，先拆成 4KiB 再撤。
        state
            .retired_cells
            .try_reserve(1)
            .map_err(|_| NestError::OutOfMemory)?;
        state
            .pending_mappings
            .try_reserve(1)
            .map_err(|_| NestError::OutOfMemory)?;
        {
            let NestState {
                root,
                retired_tables,
                ..
            } = &mut *state;
            root.prepare_geometry(address, PAGE_SIZE, self.geometry)?;
            root.reserve_retired_tables(retired_tables)?;
        }

        // 过了这里只剩下已经定好的状态转移：PTE、发布记录与承载一起转入退休责任。
        {
            let NestState {
                root,
                retired_tables,
                ..
            } = &mut *state;
            root.unmap_prepared_geometry(address, PAGE_SIZE, self.geometry);
            root.reclaim_empty_tables(retired_tables);
        }
        state.pending_mappings.push(mapping.clone());
        let (_, cell) = inner.published.remove(at);
        state.retired_cells.push(cell.retire());
        drop(inner);
        state.mark_dirty(false);
        drop(state);
        // 退休的承载只在这次确认之后才随 `finish_invalidation` 放掉。
        self.finish_invalidation()?;
        Ok(())
    }
}
