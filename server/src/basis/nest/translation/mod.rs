//! Locked CPU translation and the obligations it retains until a fence.

mod external;
mod fault;
mod target;
use target::Storage;
mod invalidation;
pub(super) mod phase;
mod retirement;
mod tickets;
pub(super) use invalidation::Invalidation;

pub use fault::FaultOutcome;

use alloc::sync::Arc;

use super::address::span::Protection;
use super::memo::publication::Publication;
use super::{Access, Span};
use super::{Asid, NestError};
use crate::basis::cell::page::{Change, Page};
use crate::metal::generic::mmu::PageTable;
use crate::metal::mmu::Geometry;
use kernel_api::address::VirtAddr;
use kernel_api::address::{PAGE_SIZE, PhysAddr};
#[cfg(debug_assertions)]
use {super::Nest, crate::metal::generic::mmu::Attributes, kernel_api::address::PageOffset};

use crate::metal::generic::mmu::fence::Completed;
use crate::metal::generic::mmu::fence::Request;
use crate::metal::mmu::Table;
use phase::ClosePhase;

use self::retirement::{Fenced, Retirement};
use super::index::Index;

/// 地址空间里除每关系库存外的全部状态：根页表、关系索引、
/// 待失效标记、退休中的资源与尚未结算的占用票。
pub(super) struct NestState {
    pub(super) root: Storage,
    pub(super) memos: super::memo::Memos,
    pub(super) index: Index,
    pub(super) retirement: Retirement,
    pub(super) phase: ClosePhase,
    pub(super) team: Option<kernel_api::model::identity::TeamId>,
}

impl NestState {
    /// 新地址空间：根页表已建，无关系、无待失效资源。
    pub(super) fn new(root: Table) -> Self {
        Self {
            root: Storage::Cpu(root),
            memos: super::memo::Memos::new(),
            index: Index::new(),
            retirement: Retirement::new(),
            phase: ClosePhase::Open,
            team: None,
        }
    }

    pub(super) fn is_open(&self) -> bool {
        self.phase.is_open()
            && match &self.root {
                Storage::Cpu(_) => true,
                Storage::External(external) => external.open(),
            }
    }

    /// Complete the current fence and close the address space if needed.
    pub(super) fn settle(&mut self, proof: Completed<Request<Asid>>) -> Result<Fenced, NestError> {
        let fenced = self.retirement.complete(proof, &mut self.memos)?;
        self.phase = self.phase.settle();
        Ok(fenced)
    }

    pub(super) fn prepare_withdrawal(
        &mut self,
        start: VirtAddr,
        size: usize,
        geometry: Geometry,
    ) -> Result<(), NestError> {
        self.root
            .cpu_mut()?
            .prepare_geometry(start, size, geometry)?;
        self.root
            .cpu_mut()?
            .reserve_retired_tables(&mut self.retirement.tables)?;
        Ok(())
    }

    pub(super) fn withdraw_prepared(&mut self, start: VirtAddr, size: usize, geometry: Geometry) {
        self.root
            .cpu_mut()
            .expect("prepared CPU withdrawal")
            .unmap_prepared_geometry(start, size, geometry);
        self.root
            .cpu_mut()
            .expect("prepared CPU withdrawal")
            .reclaim_empty_tables(&mut self.retirement.tables);
    }

    /// Withdraw the indexed mappings in one locked transition and retain
    /// their obligations until the covering fence succeeds.
    pub(super) fn close_mappings(&mut self, geometry: Geometry) -> Result<(), NestError> {
        if !self.retirement.invalidation().is_clean() {
            return Err(NestError::Busy);
        }
        // One indexed Span per service turn. Its admitted Page/active-range
        // geometry bounds the backend work; no complete-Nest snapshot is made.
        self.phase = ClosePhase::Draining;
        let mappings = self.index.first_overlap(0, usize::MAX);
        let cell_count = mappings
            .iter()
            .map(|mapping| self.memos.published(mapping).count())
            .sum();
        self.retirement
            .cells
            .try_reserve(cell_count)
            .map_err(|_| NestError::OutOfMemory)?;
        self.retirement
            .mappings
            .try_reserve(usize::from(mappings.is_some()))
            .map_err(|_| NestError::OutOfMemory)?;
        {
            if let Some(mapping) = &mappings {
                for range in &mapping.state.lock().active {
                    self.root.cpu_mut()?.prepare_geometry(
                        range.start,
                        range.size.get(),
                        geometry,
                    )?;
                }
            }
            self.root
                .cpu_mut()?
                .reserve_retired_tables(&mut self.retirement.tables)?;
            if let Some(mapping) = &mappings {
                for range in &mapping.state.lock().active {
                    self.root.cpu_mut()?.unmap_prepared_geometry(
                        range.start,
                        range.size.get(),
                        geometry,
                    );
                }
            }
            self.root
                .cpu_mut()?
                .reclaim_empty_tables(&mut self.retirement.tables);
        }
        let global = mappings.iter().any(|mapping| mapping.is_global());
        for mapping in mappings.iter() {
            self.index.remove(mapping);
            self.retirement.mappings.push(mapping.clone());
            let mut inner = mapping.state.lock();
            self.memos
                .withdraw(&mapping, 0, usize::MAX, &mut self.retirement.cells);
            inner.active.clear();
        }
        self.retirement.mark_dirty(global);
        if self.index.len() == 0 {
            self.phase = self.phase.begin();
        }
        Ok(())
    }

    #[cfg(debug_assertions)]
    pub(super) fn audit(&self, owner: &Nest, geometry: Geometry) {
        self.index.audit();
        self.index.audit_reservations(
            owner,
            self.root.cpu().expect("CPU audit"),
            geometry,
            &self.memos,
        );
        self.root.cpu().expect("CPU audit").visit_leaves(
            geometry,
            &mut |start, physical, flags, size| {
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
                    "hardware leaf crosses Span identity"
                );
                let inner = mapping.state.lock();
                assert!(
                    inner.covers(start.as_usize(), end),
                    "hardware leaf crosses an unmapped Span range"
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
                    "hardware PTE modifiability differ from Span state at {start:?}"
                );
                if let Some(source) = inner.source.as_ref().filter(|source| source.is_borrowed()) {
                    let offset = start
                        .as_usize()
                        .checked_sub(mapping.initial_start().as_usize())
                        .expect("borrowed leaf precedes Span start");
                    let page = source
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
                    let last_page = first_page + pages;
                    let mut count = 0;
                    for (page, cell) in self.memos.published(&mapping) {
                        let from = page.max(first_page);
                        let end = (page + cell.cell().pages()).min(last_page);
                        if from >= end {
                            continue;
                        }
                        assert_eq!(from, first_page + count, "published Cell index gap");
                        assert_eq!(
                            cell.physical_address() + (from - page) * PAGE_SIZE,
                            physical.as_usize() + count * PAGE_SIZE,
                            "published Cell physical address"
                        );
                        count += end - from;
                    }
                    assert_eq!(
                        count, pages,
                        "hardware leaf has pages without published Memos"
                    );
                }
            },
        );
    }

    pub(super) fn publish_page(
        &mut self,
        geometry: Geometry,
        mapping: &Arc<Span>,
        address: VirtAddr,
        page: &Arc<Page>,
        change: &Change,
        maintenance: Option<&crate::basis::cell::page::Maintenance>,
    ) -> Result<(), NestError> {
        if !self.retirement.invalidation().is_clean() {
            return Err(NestError::Busy);
        }
        if !self.is_open() {
            return Err(NestError::NoRegion);
        }
        let inner = mapping.state.lock();
        if inner.phase != super::address::span::ReservationPhase::Open {
            return Err(NestError::NoRegion);
        }
        let index = mapping.page_index(address);
        if self.memos.publication(mapping, index).is_some() {
            return Err(NestError::AlreadyMapped);
        }
        let range = inner
            .active
            .iter()
            .find(|range| range.contains(address.as_usize()))
            .ok_or(NestError::NoRegion)?;
        let flags = mapping.leaf_attributes(range.current);
        let retained = if inner.source.is_some() {
            range.ceiling
        } else {
            range.current
        };
        let mut memo = Publication::prepare_access(
            page,
            retained.contains(Access::WRITE),
            Some(change),
            maintenance,
        )?;
        self.memos.reserve(1)?;
        self.retirement
            .mappings
            .try_reserve(1)
            .map_err(|_| NestError::OutOfMemory)?;
        let physical = PhysAddr::from_raw(page.physical_address());
        self.root
            .cpu_mut()?
            .prepare_range_geometry(address, physical, page.size(), geometry)?;
        if let Some(position) = self.memos.existing_mut(mapping, index) {
            position.attempted();
        }
        memo.publish();
        self.root.cpu_mut()?.map_range_prepared_geometry(
            address,
            physical,
            page.size(),
            flags,
            geometry,
        );
        self.memos.publish(mapping, index, memo);
        self.retirement.mappings.push(mapping.clone());
        self.retirement.mark_dirty(mapping.is_global());
        Ok(())
    }

    /// Returns true when a published page was withdrawn and needs a fence.
    pub(super) fn unpublish_page(
        &mut self,
        geometry: Geometry,
        mapping: &Arc<Span>,
        address: VirtAddr,
        expected: &Arc<Page>,
    ) -> Result<bool, NestError> {
        if !self.retirement.invalidation().is_clean() {
            return Err(NestError::Shootdown);
        }
        let inner = mapping.state.lock();
        let page = mapping.page_index(address);
        let Some(publication) = self.memos.publication(mapping, page) else {
            return Ok(false);
        };
        if self.phase.is_closed() {
            return Err(NestError::Pinned);
        }
        // 核对物理 Page 实例，不能只比较 PA 或逻辑槽；检查先于任何 PTE 撤销。
        if !Arc::ptr_eq(publication.cell(), expected) {
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
        self.prepare_withdrawal(address, expected.size(), geometry)?;

        // 过了这里只剩下已经定好的状态转移：PTE、发布记录与承载一起转入退休责任。
        self.withdraw_prepared(address, expected.size(), geometry);
        self.retirement.mappings.push(mapping.clone());
        let cell = self
            .memos
            .take_publication(mapping, page)
            .expect("published Memo");
        self.retirement.cells.push(cell.retire());
        drop(inner);
        self.retirement.mark_dirty(false);
        Ok(true)
    }

    pub(super) fn protect(
        &mut self,
        geometry: Geometry,
        mapping: &Arc<Span>,
        window: (usize, usize),
        request: Protection,
        replacement: Option<(usize, Publication)>,
        accepted: &mut impl FnMut(),
    ) -> Result<(), NestError> {
        if !self.retirement.invalidation().is_clean() {
            return Err(NestError::Busy);
        }
        if !self.is_open() {
            return Err(NestError::NoRegion);
        }
        let mut inner = mapping.state.lock();
        if inner.phase != super::address::span::ReservationPhase::Open {
            return Err(NestError::NoRegion);
        }
        let transition =
            inner.protection_transition(mapping.initial_start(), window, request, |p| {
                self.memos
                    .publication(mapping, p / mapping.grain() * mapping.grain())
                    .is_some()
            })?;

        // 所有可失败的页表准备先完成；此阶段不提交新的逻辑权限或叶子权限。
        for update in &transition.updates {
            self.root
                .cpu_mut()?
                .prepare_geometry(update.address, PAGE_SIZE, geometry)?;
            self.root
                .cpu()?
                .walk_ref_geometry(update.address, geometry)?;
        }
        self.retirement
            .mappings
            .try_reserve(1)
            .map_err(|_| NestError::OutOfMemory)?;

        if let Some((page, _)) = &replacement {
            if self.memos.publication(mapping, *page).is_none() {
                return Err(NestError::Busy);
            }
            self.retirement
                .cells
                .try_reserve(1)
                .map_err(|_| NestError::OutOfMemory)?;
        }
        // 从此处开始提交不再分配：即使 shootdown 失败，保留映射与 Dirty/Failed 状态。
        accepted();
        self.retirement.mappings.push(mapping.clone());
        if let Some((page, mut publication)) = replacement {
            let old = self
                .memos
                .take_publication(mapping, page)
                .expect("prepared publication");
            publication.publish();
            self.memos.publish(mapping, page, publication);
            self.retirement.cells.push(old.retire());
        }
        for update in transition.updates {
            if let Some(memo) = self
                .memos
                .existing_mut(mapping, mapping.page_index(update.address))
            {
                memo.record_access(update.access.bits() << 1);
            }
            self.root.cpu_mut()?.set_permissions_prepared_geometry(
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
