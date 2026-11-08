//! Locked CPU translation and the obligations it retains until a fence.

mod fault;
mod invalidation;
mod phase;
mod retirement;
mod tickets;
pub(super) use invalidation::Invalidation;

pub use fault::{FaultKind, FaultOutcome};

use alloc::sync::Arc;
use alloc::vec::Vec;

use super::mapping::Protection;
use super::mapping::publication::Publication;
use super::{Access, Mapping};
use super::{Asid, NestError};
use crate::basis::cell::page::{Change, Page};
use crate::machine::mmu::{Geometry, PageTable};
use kernel_api::address::VirtAddr;
use kernel_api::address::{PAGE_SIZE, PhysAddr};
#[cfg(debug_assertions)]
use {super::Nest, crate::machine::mmu::Attributes, kernel_api::address::PageOffset};

use crate::machine::mmu::Table;
use crate::machine::mmu::fence::Completed;
use phase::ClosePhase;

use self::retirement::{Fenced, Retirement};
use super::index::Index;

/// 地址空间里除每关系库存外的全部状态：根页表、关系索引、
/// 待失效标记、退休中的资源与尚未结算的占用票。
pub(super) struct NestState {
    pub(super) root: Table,
    pub(super) index: Index,
    pub(super) retirement: Retirement,
    pub(super) phase: ClosePhase,
}

impl NestState {
    /// 新地址空间：根页表已建，无关系、无待失效资源。
    pub(super) fn new(root: Table) -> Self {
        Self {
            root,
            index: Index::new(),
            retirement: Retirement::new(),
            phase: ClosePhase::Open,
        }
    }

    /// Complete the current fence and close the address space if needed.
    pub(super) fn settle(&mut self, asid: Asid, proof: Completed<Asid>) -> Fenced {
        let fenced = self.retirement.complete(asid, proof);
        self.phase = self.phase.settle();
        fenced
    }

    pub(super) fn prepare_withdrawal(
        &mut self,
        start: VirtAddr,
        size: usize,
        geometry: Geometry,
    ) -> Result<(), NestError> {
        self.root.prepare_geometry(start, size, geometry)?;
        self.root
            .reserve_retired_tables(&mut self.retirement.tables)?;
        Ok(())
    }

    pub(super) fn withdraw_prepared(&mut self, start: VirtAddr, size: usize, geometry: Geometry) {
        self.root.unmap_prepared_geometry(start, size, geometry);
        self.root.reclaim_empty_tables(&mut self.retirement.tables);
    }

    /// Withdraw the indexed mappings in one locked transition and retain
    /// their obligations until the covering fence succeeds.
    pub(super) fn close_mappings(&mut self, geometry: Geometry) -> Result<(), NestError> {
        let mut mappings = Vec::new();
        mappings
            .try_reserve(self.index.len())
            .map_err(|_| NestError::OutOfMemory)?;
        self.index.collect(&mut mappings);
        let cell_count = mappings
            .iter()
            .map(|mapping| mapping.state.lock().published.len())
            .sum();
        self.retirement
            .cells
            .try_reserve(cell_count)
            .map_err(|_| NestError::OutOfMemory)?;
        self.retirement
            .mappings
            .try_reserve(mappings.len())
            .map_err(|_| NestError::OutOfMemory)?;
        {
            for mapping in &mappings {
                for range in &mapping.state.lock().active {
                    self.root
                        .prepare_geometry(range.start, range.size.get(), geometry)?;
                }
            }
            self.root
                .reserve_retired_tables(&mut self.retirement.tables)?;
            for mapping in &mappings {
                for range in &mapping.state.lock().active {
                    self.root
                        .unmap_prepared_geometry(range.start, range.size.get(), geometry);
                }
            }
            self.root.reclaim_empty_tables(&mut self.retirement.tables);
        }
        let global = mappings.iter().any(|mapping| mapping.is_global());
        for mapping in mappings {
            self.retirement.mappings.push(mapping.clone());
            let mut inner = mapping.state.lock();
            for (_, cell) in inner.published.drain(..) {
                self.retirement.cells.push(cell.retire());
            }
            inner.active.clear();
        }
        self.index.clear();
        self.retirement.mark_dirty(global);
        self.phase = self.phase.begin();
        Ok(())
    }

    #[cfg(debug_assertions)]
    pub(super) fn audit(&self, owner: &Nest, geometry: Geometry) {
        self.index.audit();
        self.index.audit_mappings(owner, &self.root, geometry);
        self.root
            .visit_leaves(geometry, &mut |start, physical, flags, size| {
                let end = start
                    .as_usize()
                    .checked_add(size - 1)
                    .expect("leaf virtual interval is representable");
                let mapping = self
                    .index
                    .find(start.as_usize())
                    .expect("hardware leaf is absent from the nest index");
                let end_mapping = self
                    .index
                    .find(end)
                    .expect("hardware leaf extends beyond the nest index");
                assert!(
                    Arc::ptr_eq(&mapping, &end_mapping),
                    "hardware leaf crosses Mapping identity"
                );
                let inner = mapping.state.lock();
                assert!(
                    inner.covers(start.as_usize(), end),
                    "hardware leaf crosses an unmapped Mapping range"
                );
                let mut expected: Option<Attributes> = None;
                for range in inner
                    .active
                    .iter()
                    .filter(|range| range.overlaps(start.as_usize(), end))
                {
                    let leaf = mapping.leaf_attributes(range.current);
                    if let Some(expected) = expected {
                        assert_eq!(
                            leaf.bits(),
                            expected.bits(),
                            "hardware leaf crosses modifiable state"
                        );
                    } else {
                        expected = Some(leaf);
                    }
                }
                let expected = expected.expect("hardware leaf belongs to a live range");
                let mask = Attributes::READ
                    | Attributes::WRITE
                    | Attributes::EXECUTE
                    | Attributes::USER
                    | Attributes::GLOBAL;
                assert_eq!(
                    (flags & mask).bits(),
                    (expected & mask).bits(),
                    "hardware PTE modifiability differ from Mapping state at {start:?}"
                );
                if inner.source.is_borrowed() {
                    let offset = start
                        .as_usize()
                        .checked_sub(mapping.initial_start().as_usize())
                        .expect("borrowed leaf precedes Mapping start");
                    let page = inner
                        .source
                        .materialize()
                        .expect("borrowed source materializes")
                        .physical(PageOffset::new(offset / PAGE_SIZE))
                        .expect("borrowed leaf lies inside the source range");
                    assert_eq!(
                        physical.as_usize(),
                        page.as_usize(),
                        "borrowed PTE physical address differs"
                    );
                } else {
                    let first_page = mapping.page_index(start);
                    let pages = size / PAGE_SIZE;
                    let first = inner
                        .published
                        .partition_point(|(page, _)| *page < first_page);
                    let last_page = first_page + pages;
                    let last = inner
                        .published
                        .partition_point(|(page, _)| *page < last_page);
                    assert_eq!(
                        last - first,
                        pages,
                        "hardware leaf has pages without published Cells"
                    );
                    for (offset, (page, cell)) in inner.published[first..last].iter().enumerate() {
                        assert_eq!(
                            *page,
                            first_page + offset,
                            "published Page index gap under hardware leaf"
                        );
                        assert_eq!(
                            cell.physical_address(),
                            physical.as_usize() + offset * PAGE_SIZE,
                            "published Page physical address differs from hardware PTE"
                        );
                    }
                }
            });
    }

    pub(super) fn publish_page(
        &mut self,
        geometry: Geometry,
        mapping: &Arc<Mapping>,
        address: VirtAddr,
        page: &Arc<Page>,
        change: &Change,
    ) -> Result<(), NestError> {
        let mut inner = mapping.state.lock();
        let index = mapping.page_index(address);
        if inner.publishes(index) {
            return Err(NestError::AlreadyMapped);
        }
        let range = inner
            .active
            .iter()
            .find(|range| range.contains(address.as_usize()))
            .ok_or(NestError::NoRegion)?;
        let flags = mapping.leaf_attributes(range.current);
        let mut memo =
            Publication::prepare(page, range.ceiling.contains(Access::WRITE), Some(change))?;
        inner
            .published
            .try_reserve(1)
            .map_err(|_| NestError::OutOfMemory)?;
        self.retirement
            .mappings
            .try_reserve(1)
            .map_err(|_| NestError::OutOfMemory)?;
        let physical = PhysAddr::from_raw(page.physical_address());
        self.root
            .prepare_range_geometry(address, physical, PAGE_SIZE, geometry)?;
        memo.publish();
        self.root
            .map_range_prepared_geometry(address, physical, PAGE_SIZE, flags, geometry);
        inner.publish(index, memo);
        self.retirement.mappings.push(mapping.clone());
        self.retirement.mark_dirty(mapping.is_global());
        Ok(())
    }

    /// Returns true when a published page was withdrawn and needs a fence.
    pub(super) fn unpublish_page(
        &mut self,
        geometry: Geometry,
        mapping: &Arc<Mapping>,
        address: VirtAddr,
        expected: &Arc<Page>,
    ) -> Result<bool, NestError> {
        if !self.retirement.invalidation().is_clean() {
            return Err(NestError::Shootdown);
        }
        let mut inner = mapping.state.lock();
        let page = mapping.page_index(address);
        let Some(at) = inner.published.iter().position(|(held, _)| *held == page) else {
            return Ok(false);
        };
        if self.phase.is_closed() {
            return Err(NestError::Pinned);
        }
        // 核对物理 Page 实例，不能只比较 PA 或逻辑槽；检查先于任何 PTE 撤销。
        if !Arc::ptr_eq(inner.published[at].1.cell(), expected) {
            return Err(NestError::Busy);
        }
        debug_assert!(
            inner.covers(address.as_usize(), address.as_usize()),
            "a published mapping lies inside a live range"
        );
        // 提前备容量与几何：这一页可能只是大叶里的一格，先拆成 4KiB 再撤。
        self.retirement
            .cells
            .try_reserve(1)
            .map_err(|_| NestError::OutOfMemory)?;
        self.retirement
            .mappings
            .try_reserve(1)
            .map_err(|_| NestError::OutOfMemory)?;
        self.prepare_withdrawal(address, PAGE_SIZE, geometry)?;

        // 过了这里只剩下已经定好的状态转移：PTE、发布记录与承载一起转入退休责任。
        self.withdraw_prepared(address, PAGE_SIZE, geometry);
        self.retirement.mappings.push(mapping.clone());
        let (_, cell) = inner.published.remove(at);
        self.retirement.cells.push(cell.retire());
        drop(inner);
        self.retirement.mark_dirty(false);
        Ok(true)
    }

    pub(super) fn protect(
        &mut self,
        geometry: Geometry,
        mapping: &Arc<Mapping>,
        window: (usize, usize),
        request: Protection,
    ) -> Result<(), NestError> {
        let mut inner = mapping.state.lock();
        let transition = inner.protection_transition(mapping.initial_start(), window, request)?;

        // 所有可失败的页表准备先完成；此阶段不提交新的逻辑权限或叶子权限。
        for update in &transition.updates {
            self.root
                .prepare_geometry(update.address, PAGE_SIZE, geometry)?;
            self.root.walk_ref_geometry(update.address, geometry)?;
        }
        self.retirement
            .mappings
            .try_reserve(1)
            .map_err(|_| NestError::OutOfMemory)?;

        // 从此处开始提交不再分配：即使 shootdown 失败，保留映射与 Dirty/Failed 状态。
        self.retirement.mappings.push(mapping.clone());
        for update in transition.updates {
            self.root.set_permissions_prepared_geometry(
                update.address,
                mapping.leaf_attributes(update.access),
                geometry,
            );
        }
        inner.active = transition.active;
        drop(inner);
        self.index.refresh(mapping);
        self.retirement.mark_dirty(mapping.is_global());
        Ok(())
    }
}
