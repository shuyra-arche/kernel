//! CPU 翻译：地址占用、翻译关系、页表、身份与同步退休。
//!
//! Flat 继续独立管理来源与物化；这里不保存来源页槽。

mod access;
mod address;
mod error;
mod index;
pub(crate) mod mapping;
mod translation;
pub(crate) mod wait;

use alloc::boxed::Box;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::mem::ManuallyDrop;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering;

use crate::basis::cell::page::{Change, Page};
use crate::basis::flat::Pages;
use crate::basis::sign::{self, WakeKey};
use crate::basis::unit::anchor::Anchor;
use crate::metal::layout::TEAM_FRAME_BASE;
use crate::metal::layout::TEAM_FRAME_WINDOW_SIZE;
use crate::metal::layout::TRAP_STACK_BASE;
use crate::metal::mmu::{self, Attributes, Geometry, PageTable, Root, Table, identity};
use crate::metal::sync::Level;
use crate::metal::sync::SpinLock;
use crate::metal::sync::SpinLockGuard;
use kernel_api::address::PAGE_SIZE;
use kernel_api::address::PageCount;
use kernel_api::address::PageOffset;
use kernel_api::address::PhysAddr;
use kernel_api::address::VirtAddr;
use translation::Invalidation;

pub use address::Reservation;
pub use error::NestError;
pub use identity::Asid;
pub use mapping::{Access, ActiveRange, Mapping};
pub use translation::{FaultKind, FaultOutcome};

use address::Layout;
pub(crate) use address::{Placement, PlacementAt, RegionKind};
pub(crate) use mapping::MappingRange;

use index::IndexNode;
use mapping::{publication::Publication, source};

use translation::NestState;

/// 待清理链条上的一环：一笔已经消费、但还没收尾的关系。
///
/// 节点在发布前预留下来，接管回收时不再分配；
/// 它只保存关系的强持，占用票（退休）与 Page / 表页的账在 Nest 自己那里。
struct CleanupNode {
    mapping: Option<Arc<Mapping>>,
    next: Option<Box<CleanupNode>>,
}

impl CleanupNode {
    const fn empty() -> Self {
        Self {
            mapping: None,
            next: None,
        }
    }
}

/// 一个 CPU 地址空间：身份、根页表、全局占用、关系与同步退休。
///
/// 关系（`Mapping`）自己持有来源与活动库存；这里只保存它们的索引、
/// 待失效标记和退休中的资源。占用表只登记跨度，不物化、不发布。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct NestId(usize);
impl NestId {
    pub(crate) fn get(self) -> usize {
        self.0
    }
    fn allocate() -> Result<Self, NestError> {
        static NEXT: crate::basis::name::Serial = crate::basis::name::Serial::new(1, usize::MAX);
        NEXT.allocate().map(Self).ok_or(NestError::OutOfMemory)
    }
}

pub struct Nest {
    id: NestId,
    asid: Asid,
    kind: SpaceKind,
    geometry: Geometry,
    life: Arc<Anchor>,
    pub(crate) mappings: mapping::MappingRegistry,
    layout: SpinLock<Layout>,
    pending_cleanup: SpinLock<Option<Box<CleanupNode>>>,
    state: ManuallyDrop<SpinLock<NestState>>,
    asid_released: AtomicBool,
    published: AtomicBool,
}

impl Nest {
    /// 用户地址空间：动态取一个 ASID，窗口按平台布局登记。
    pub(crate) fn new(kind: SpaceKind) -> Result<Arc<Self>, NestError> {
        Self::build(kind, false)
    }

    /// 内核地址空间：ASID 0 固定，`Resource` 区可用，生命周期为进程期。
    pub(crate) fn kernel() -> Result<Arc<Self>, NestError> {
        Self::build(SpaceKind::Supervisor, true)
    }

    fn build(kind: SpaceKind, kernel: bool) -> Result<Arc<Self>, NestError> {
        let root = Table::root()?;
        let mut layout = Layout::new();
        layout.register_window(
            RegionKind::Normal,
            crate::metal::layout::IMAGE_BASE.as_usize(),
            mmu::upper().as_usize() - 1,
        )?;
        layout.register_window(
            RegionKind::Kernel,
            TEAM_FRAME_BASE.as_usize(),
            TEAM_FRAME_BASE.as_usize() + TEAM_FRAME_WINDOW_SIZE - 1,
        )?;
        if kernel {
            let dram = crate::metal::board::info().memory.envelope();
            let base = mmu::lower()
                .as_usize()
                .checked_add(dram.base)
                .and_then(|base| base.checked_add(dram.size))
                .ok_or(NestError::DramOverlap)?;
            layout.register_window(RegionKind::Resource, base, TRAP_STACK_BASE.as_usize() - 1)?;
        }
        let life = Anchor::try_new()?;
        let id = NestId::allocate()?;
        let asid = if kernel {
            Asid::kernel()
        } else {
            Asid::try_allocate().ok_or(NestError::OutOfMemory)?
        };
        let mut nest = Arc::try_new(Self {
            id,
            asid,
            kind,
            geometry: mmu::geometry(),
            life,
            mappings: mapping::MappingRegistry::new(),
            layout: SpinLock::new_level(Level::Address, layout),
            pending_cleanup: SpinLock::new_level(Level::Address, None),
            state: ManuallyDrop::new(SpinLock::new_level(Level::Nest, NestState::new(root))),
            asid_released: AtomicBool::new(kernel),
            published: AtomicBool::new(false),
        })
        .map_err(|_| NestError::OutOfMemory)?;
        Arc::get_mut(&mut nest)
            .expect("new Arc is unique")
            .published
            .store(true, Ordering::Release);
        Ok(nest)
    }

    /// 尚未发布的事务只在这些阶段可见：Clean 且未关闭。
    fn ready_state(&self) -> Result<SpinLockGuard<'_, NestState>, NestError> {
        loop {
            let state = self.state.lock();
            if state.phase.is_closed() {
                return Err(NestError::NoRegion);
            }
            match state.retirement.invalidation() {
                Invalidation::Clean => return Ok(state),
                Invalidation::Failed => return Err(NestError::Shootdown),
                Invalidation::Dirty | Invalidation::Fencing => {}
            }
            drop(state);
            self.finish_invalidation()?;
        }
    }

    pub fn kind(&self) -> SpaceKind {
        self.kind
    }

    pub(crate) fn life(&self) -> Weak<Anchor> {
        Arc::downgrade(&self.life)
    }

    pub(crate) fn id(&self) -> NestId {
        self.id
    }

    pub(crate) fn asid(&self) -> Asid {
        self.asid
    }

    pub(crate) fn root(&self) -> Root {
        Root {
            frame: self.state.lock().root.frame(),
            asid: self.asid,
        }
    }

    /// 是否有活关系与 `[start, start + size)` 相交。
    pub(crate) fn overlaps(&self, start: VirtAddr, size: usize) -> Result<bool, NestError> {
        if size == 0 || start.offset() != 0 || !size.is_multiple_of(PAGE_SIZE) {
            return Err(NestError::NotAligned);
        }
        let end = start
            .as_usize()
            .checked_add(size - 1)
            .ok_or(NestError::NotAligned)?;
        let state = self.state.lock();
        Ok(state.index.first_overlap(start.as_usize(), end).is_some())
    }

    pub(crate) fn has_pending_invalidation(&self) -> bool {
        !self.state.lock().retirement.invalidation().is_clean()
    }

    pub(crate) fn contains_reservation(&self, id: usize) -> bool {
        self.layout.lock().occupancy.has_id(id)
    }

    /// 原身份的占用票已撤下，或全空间关闭的失效同步已完成。
    pub(crate) fn settled(&self, id: usize) -> bool {
        self.is_closed() || !self.contains_reservation(id)
    }

    /// 普通挂载的架构属性：用户空间只对 `Normal` 区开 U，A/D 由内核自维护。
    pub(crate) fn attrs(&self, region: Option<RegionKind>) -> Attributes {
        let mut attrs = Attributes::ACCESSED | Attributes::DIRTY;
        if !self.kind.is_supervisor() && region == Some(RegionKind::Normal) {
            attrs |= Attributes::USER;
        }
        attrs
    }

    /// 抬高窗口的选址下界；已登记的占用不动。
    pub(crate) fn advance_window(&self, region: RegionKind, base: usize) -> Result<(), NestError> {
        Ok(self.layout.lock().advance_window(region, base)?)
    }

    /// 该区域的动态窗口是否含住 `[start, start + size)`。
    pub(crate) fn window_contains(&self, region: RegionKind, start: VirtAddr, size: usize) -> bool {
        let Some(last) = start.as_usize().checked_add(size - 1) else {
            return false;
        };
        self.layout
            .lock()
            .window_contains(region, start.as_usize(), last)
    }

    /// 选一段跨度并登记占用；不动页表、不物化来源。
    pub(crate) fn reserve(
        self: &Arc<Self>,
        placement: Placement,
    ) -> Result<Reservation, NestError> {
        let (start, id) = self.layout.lock().reserve(placement)?;
        Ok(Reservation::new(
            Arc::downgrade(self),
            placement.region(),
            crate::metal::mmu::canonical(start),
            placement.size(),
            id,
        ))
    }

    /// 普通挂载：整段初始范围一条活动记录，属性按种类与区域政策算出。
    pub(crate) fn attach(
        self: &Arc<Self>,
        reservation: Reservation,
        source: crate::basis::flat::FlatRange,
        ceiling: Access,
        current: Access,
    ) -> Result<Arc<Mapping>, NestError> {
        if let Err(error) = self.check_owner(&reservation) {
            return Err(Self::abandon(reservation, error));
        }
        let attrs = self.attrs(reservation.region());
        let active =
            match ActiveRange::new(reservation.start(), reservation.size(), ceiling, current) {
                Ok(active) => [active],
                Err(error) => return Err(Self::abandon(reservation, error)),
            };
        self.attach_shaped(reservation, source, attrs, &active)
    }

    /// Reserve an explicit Memo collection without making any address accessible.
    pub(crate) fn create_memo(
        self: &Arc<Self>,
        reservation: Reservation,
        source: crate::basis::flat::FlatRange,
        ceiling: Access,
        current: Access,
    ) -> Result<Arc<Mapping>, NestError> {
        if let Err(error) = self.check_owner(&reservation) {
            return Err(Self::abandon(reservation, error));
        }
        if source.is_borrowed() {
            return Err(Self::abandon(reservation, NestError::Pinned));
        }
        let attrs = self.attrs(reservation.region());
        let active =
            match ActiveRange::new(reservation.start(), reservation.size(), ceiling, current) {
                Ok(active) => active,
                Err(error) => return Err(Self::abandon(reservation, error)),
            };
        let mut ranges = Vec::new();
        if ranges.try_reserve(1).is_err() {
            return Err(Self::abandon(reservation, NestError::OutOfMemory));
        }
        ranges.push(active);
        let mapping = Mapping::new(reservation, attrs, ranges, source)?;
        mapping.state.lock().explicit = true;
        match self.commit(&mapping) {
            Ok(()) => Ok(mapping),
            Err(error) => {
                self.settle_failed_install(&mapping);
                Err(error)
            }
        }
    }

    /// 按给定活动形状挂载：与普通入口共用一个私有安装/提交。
    ///
    /// 只接真实占用、真实来源与明确的活动区间；不接来源关系、
    /// 不接第二份可伪造的地址投影。占用必须是本 Nest 的（否则 `NoRegion`），
    /// 未发布的失败退回它的属主，已发布的失败进清理链。
    pub(crate) fn attach_shaped(
        self: &Arc<Self>,
        reservation: Reservation,
        source: crate::basis::flat::FlatRange,
        attrs: Attributes,
        active: &[ActiveRange],
    ) -> Result<Arc<Mapping>, NestError> {
        if let Err(error) = self.check_owner(&reservation) {
            return Err(Self::abandon(reservation, error));
        }
        if !Mapping::attrs_are_architectural(attrs) {
            return Err(Self::abandon(reservation, NestError::NotAligned));
        }
        let mut ranges = Vec::new();
        if ranges.try_reserve(active.len()).is_err() {
            return Err(Self::abandon(reservation, NestError::OutOfMemory));
        }
        ranges.extend_from_slice(active);
        let mapping = Mapping::new(reservation, attrs, ranges, source)?;
        match self.commit(&mapping) {
            Ok(()) => Ok(mapping),
            Err(error) => {
                self.settle_failed_install(&mapping);
                Err(error)
            }
        }
    }

    /// 一笔尚未发布的占用的归还方向：它的属主自己的表，不是接收调用的那一方。
    ///
    /// 占用身份是全局唯一的，所以“拿裸坐标去目标表里找一笔”不会误删别人的槽；
    /// 但那样也退不掉真属主的这一笔，因此归还一律按属主。
    fn give_back_unpublished(reservation: &Reservation) -> bool {
        reservation
            .nest()
            .is_some_and(|owner| owner.give_back(reservation))
    }

    /// 占用已经交出、本笔操作不再有承接者时唯一的归还路径：按属主交还并交出错误。
    ///
    /// 与调用方主动退的 `release_reservation` 成对；`give_back` / `give_back_span`
    /// 只是“退还”的原语，不包含“这笔操作失败”的语义。
    fn abandon(reservation: Reservation, error: NestError) -> NestError {
        Self::give_back_unpublished(&reservation);
        error
    }

    /// 安装失败的责任划分：已发布就把回收交给清理链（占用仍禁复用），
    /// 未发布就退回占用与临时数据。
    fn settle_failed_install(&self, mapping: &Arc<Mapping>) {
        let published = {
            let state = self.state.lock();
            state
                .index
                .find(mapping.initial_start().as_usize())
                .is_some_and(|held| Arc::ptr_eq(&held, mapping))
        };
        if !published {
            Self::give_back_unpublished(&mapping.reservation);
            return;
        }
        let node = mapping.take_cleanup();
        if let Some(mut node) = node {
            node.mapping = Some(mapping.clone());
            self.enqueue_cleanup(node);
        }
    }

    /// 安装/提交：所有可失败的元数据、几何与清理载荷都先备齐，
    /// 之后的 PTE 写入与索引接入不再失败。
    fn commit(self: &Arc<Self>, mapping: &Arc<Mapping>) -> Result<(), NestError> {
        let reservation = &mapping.reservation;
        let start = reservation.start();
        let end = reservation.last();
        {
            let layout = self.layout.lock();
            if !layout.occupancy.has_id(reservation.id()) {
                return Err(NestError::NoRegion);
            }
            mapping.prepare_cleanup()?;
        }
        // 本关系要发布哪些页：Managed 只取已物化的，缺口留给缺页；
        // Borrowed 的整段都可用，挂载时就全部发布。
        let mut published: Vec<(usize, Publication)> = Vec::new();
        let mut borrowed: Vec<(VirtAddr, usize, Access)> = Vec::new();
        let mut pages: Option<Pages> = None;
        // 发布窗口的短期驻留：在抓承载快照**之前**取得，覆盖 PTE 写入与发布记录提交，
        // 之后才解除。在途的移交槽会让这次驻留当场 Busy，正好与事务互斥。
        let lease = {
            let inner = mapping.state.lock();
            if inner.explicit {
                None
            } else {
                Some(inner.source.reside()?)
            }
        };
        {
            let inner = mapping.state.lock();
            if inner.explicit {
                // Association and publication are separate explicit operations.
            } else if inner.source.is_borrowed() {
                pages = Some(inner.source.materialize()?);
                borrowed
                    .try_reserve(inner.active.len())
                    .map_err(|_| NestError::OutOfMemory)?;
                for range in &inner.active {
                    borrowed.push((range.start, range.size.get(), range.current));
                }
            } else {
                let materialized = inner.source.materialized_cells()?;
                published
                    .try_reserve(materialized.len())
                    .map_err(|_| NestError::OutOfMemory)?;
                for (offset, cell) in materialized {
                    let page = offset.get();
                    let address = start + page * PAGE_SIZE;
                    if let Some(range) = inner
                        .active
                        .iter()
                        .find(|range| range.contains(address.as_usize()))
                    {
                        published.push((
                            page,
                            Publication::prepare(
                                &cell,
                                range.ceiling.contains(Access::WRITE),
                                None,
                            )?,
                        ));
                    }
                }
            }
        }
        // 发布记录容量是安装前的准备：与 PTE 写入同一批，备不齐就不进入安装。
        // 于是“失败时还没有任何 PTE”对 `settle_failed_install` 的判据成立。
        {
            let mut inner = mapping.state.lock();
            inner
                .published
                .try_reserve(published.len())
                .map_err(|_| NestError::OutOfMemory)?;
        }
        let node =
            Box::try_new(IndexNode::new(mapping.clone())?).map_err(|_| NestError::OutOfMemory)?;
        let mut state = self.ready_state()?;
        if state.index.first_overlap(start.as_usize(), end).is_some() {
            return Err(NestError::AlreadyMapped);
        }
        state
            .retirement
            .mappings
            .try_reserve(1)
            .map_err(|_| NestError::OutOfMemory)?;
        // 首条 PTE 之前登记来源↔关系：撤页要按来源枚举全部关系，不能只看用户注册表。
        source::register(&mapping.state.lock().source, mapping)?;
        for run in mapping.state.lock().runs(start, &published) {
            let run = run?;
            state
                .root
                .prepare_range_geometry(run.start, run.physical, run.size, self.geometry)?;
        }
        for (at, len, _) in &borrowed {
            let pa = pages
                .as_ref()
                .expect("borrowed runs imply a borrowed source")
                .physical(PageOffset::new(
                    (at.as_usize() - start.as_usize()) / PAGE_SIZE,
                ))?;
            state
                .root
                .prepare_range_geometry(*at, pa, *len, self.geometry)?;
        }
        // 可失败准备到此为止：这条关系随后只进不可失败的 PTE 提交，
        // 而短期驻留仍守着这份来源。内核地址空间与全局条目都让整份来源
        // 永久失去迁移资格。
        if self.asid.is_kernel() || mapping.is_global() {
            mapping.state.lock().source.mark_untransferable();
        }
        for (_, retained) in &mut published {
            retained.publish();
        }
        state.retirement.mappings.push(mapping.clone());
        for run in mapping.state.lock().runs(start, &published) {
            let run = run?;
            state.root.map_range_prepared_geometry(
                run.start,
                run.physical,
                run.size,
                mapping.leaf_attributes(run.current),
                self.geometry,
            );
        }
        for (at, len, current) in &borrowed {
            let pa = pages
                .as_ref()
                .expect("borrowed runs imply a borrowed source")
                .physical(PageOffset::new(
                    (at.as_usize() - start.as_usize()) / PAGE_SIZE,
                ))?;
            state.root.map_range_prepared_geometry(
                *at,
                pa,
                *len,
                mapping.leaf_attributes(*current),
                self.geometry,
            );
        }
        {
            let mut inner = mapping.state.lock();
            for (page, cell) in published {
                inner.publish(page, cell);
            }
        }
        state.index.insert(node);
        state.retirement.mark_dirty(mapping.is_global());
        drop(state);
        drop(lease);
        self.finish_invalidation()
    }

    /// 把该窗口里已经物化/借用的页发布到本关系：不动 Flat 的物化。
    ///
    /// 只发布活动形状内的页，不偷分配未物化的页。
    fn publish(self: &Arc<Self>, selection: MappingRange) -> Result<(), NestError> {
        let mapping = selection.mapping().clone();
        for (start, size) in self.active_spans(&selection)? {
            self.publish_span(&mapping, start, size)?;
        }
        Ok(())
    }

    fn publish_span(
        self: &Arc<Self>,
        mapping: &Arc<Mapping>,
        start: VirtAddr,
        size: usize,
    ) -> Result<(), NestError> {
        if size == 0 || start.offset() != 0 || !size.is_multiple_of(PAGE_SIZE) {
            return Err(NestError::NotAligned);
        }
        let end = start
            .as_usize()
            .checked_add(size - 1)
            .ok_or(NestError::NotAligned)?;
        let mut state = self.ready_state()?;
        self.ensure_owned(mapping, &state)?;
        let mut inner = mapping.state.lock();
        if !inner.covers(start.as_usize(), end) {
            return Err(NestError::NoRegion);
        }
        if inner.source.is_borrowed() {
            return Ok(());
        }
        let first = mapping.page_index(start);
        let window = inner.source.range(
            PageOffset::new(first),
            PageCount::new(size / PAGE_SIZE).ok_or(NestError::NotAligned)?,
        )?;
        // 发布窗口的短期驻留：在抓承载快照**之前**取得，覆盖 PTE 写入与发布记录提交。
        // 在途的移交槽会让这次驻留当场 Busy，正好与事务互斥。
        let lease = window.reside()?;
        let materialized = window.materialized_cells()?;
        let mut fresh = Vec::new();
        fresh
            .try_reserve(materialized.len())
            .map_err(|_| NestError::OutOfMemory)?;
        for (offset, cell) in materialized {
            let page = first + offset.get();
            if !inner.explicit && inner.memo(page).is_none() && !inner.publishes(page) {
                let address = mapping.reservation.start() + page * PAGE_SIZE;
                let range = inner
                    .active
                    .iter()
                    .find(|range| range.contains(address.as_usize()))
                    .ok_or(NestError::NoRegion)?;
                fresh.push((
                    page,
                    Publication::prepare(&cell, range.ceiling.contains(Access::WRITE), None)?,
                ));
            }
        }
        if fresh.is_empty() {
            return Ok(());
        }
        inner
            .published
            .try_reserve(fresh.len())
            .map_err(|_| NestError::OutOfMemory)?;
        state
            .retirement
            .mappings
            .try_reserve(1)
            .map_err(|_| NestError::OutOfMemory)?;
        for run in inner.runs(mapping.initial_start(), &fresh) {
            let run = run?;
            state
                .root
                .prepare_range_geometry(run.start, run.physical, run.size, self.geometry)?;
        }
        // 可失败准备到此为止：随后只有这一页的 PTE 提交，短期驻留仍守着这份来源。
        // 内核地址空间与全局条目都让整份来源永久失去迁移资格。
        if self.asid.is_kernel() || mapping.is_global() {
            inner.source.mark_untransferable();
        }
        for (_, retained) in &mut fresh {
            retained.publish();
        }
        state.retirement.mappings.push(mapping.clone());
        for run in inner.runs(mapping.initial_start(), &fresh) {
            let run = run?;
            state.root.map_range_prepared_geometry(
                run.start,
                run.physical,
                run.size,
                mapping.leaf_attributes(run.current),
                self.geometry,
            );
        }
        for (page, cell) in fresh {
            inner.publish(page, cell);
        }
        state.retirement.mark_dirty(mapping.is_global());
        drop(inner);
        drop(state);
        drop(lease);
        self.finish_invalidation()
    }

    pub(crate) fn memo_page(
        &self,
        mapping: &Arc<Mapping>,
        page: usize,
    ) -> Result<Option<Arc<Page>>, NestError> {
        let state = self.ready_state()?;
        self.ensure_owned(mapping, &state)?;
        let inner = mapping.state.lock();
        Ok(inner.memo_page(page))
    }

    pub(crate) fn associate(
        &self,
        mapping: &Arc<Mapping>,
        index: usize,
        expected: u64,
        page: Arc<Page>,
    ) -> Result<(), NestError> {
        let state = self.ready_state()?;
        self.ensure_owned(mapping, &state)?;
        let mut inner = mapping.state.lock();
        inner.associate(mapping.initial_start(), index, expected, page)
    }
    /// Start a qualified logical transition; hardware operations hold no Mapping lock.
    pub(crate) fn transition_memo(
        &self,
        mapping: &Arc<Mapping>,
        index: usize,
        expected: u64,
        freeze: Arc<Change>,
        publishing: bool,
    ) -> Result<Arc<Page>, NestError> {
        let state = self.ready_state()?;
        self.ensure_owned(mapping, &state)?;
        let mut inner = mapping.state.lock();
        inner.transition_memo(index, expected, freeze, publishing)
    }
    /// A failed pre-publication attempt remains Ready and keeps its freeze.
    pub(crate) fn finish_memo(&self, mapping: &Arc<Mapping>, index: usize) {
        if self.has_pending_invalidation() {
            return;
        }
        mapping.state.lock().finish_memo(index);
    }

    pub(crate) fn push_page(
        self: &Arc<Self>,
        mapping: &Arc<Mapping>,
        address: VirtAddr,
        page: &Arc<Page>,
        change: &Change,
    ) -> Result<(), NestError> {
        self.finish_invalidation()?;
        let mut state = self.ready_state()?;
        self.ensure_owned(mapping, &state)?;
        state.publish_page(self.geometry, mapping, address, page, change)?;
        drop(state);
        self.finish_invalidation()
    }

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
    pub(crate) fn unpublish_page(
        self: &Arc<Self>,
        mapping: &Arc<Mapping>,
        address: VirtAddr,
        expected: &Arc<Page>,
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

        let mut state = self.ready_state()?;
        if !state.unpublish_page(self.geometry, mapping, address, expected)? {
            return Ok(());
        }
        drop(state);
        // 退休的承载只在这次确认之后才随 `finish_invalidation` 放掉。
        self.finish_invalidation()?;
        Ok(())
    }

    /// 让 `Flat` 物化该窗口的活动页，再在本关系发布。
    ///
    /// 组合入口：物化原语仍是 `FlatRange::materialize`，这里只把缺页与派生编排
    /// 要的两步（先 Flat、再本关系 publish）接在一起，不是第二套物化权威。
    pub(crate) fn materialize_active(
        self: &Arc<Self>,
        selection: MappingRange,
    ) -> Result<(), NestError> {
        let mapping = selection.mapping().clone();
        for (start, size) in self.active_spans(&selection)? {
            let offset = PageOffset::new(mapping.page_index(start));
            let count = PageCount::new(size / PAGE_SIZE).ok_or(NestError::NotAligned)?;
            let part = mapping.range(offset, count)?;
            part.source()?.materialize()?;
            self.publish(part)?;
        }
        Ok(())
    }

    /// 发布只选择活动页，不把尚未结算的撤销重新安装。
    fn active_spans(&self, selection: &MappingRange) -> Result<Vec<(VirtAddr, usize)>, NestError> {
        let mapping = selection.mapping();
        let (lo, hi) = selection.window();
        let state = self.ready_state()?;
        self.ensure_owned(mapping, &state)?;
        let inner = mapping.state.lock();
        if inner.active.is_empty()
            || (matches!(selection, MappingRange::Pages { .. }) && !inner.covers(lo, hi))
        {
            return Err(NestError::NoRegion);
        }
        let mut spans = Vec::new();
        spans
            .try_reserve(inner.active.len())
            .map_err(|_| NestError::OutOfMemory)?;
        for range in &inner.active {
            if let Some((start, last)) = range.intersection(lo, hi) {
                spans.push((crate::metal::mmu::canonical(start), last - start + 1));
            }
        }
        Ok(spans)
    }

    /// 在权限上限内改选择窗口的当前逻辑权限；不改架构属性。
    pub(crate) fn set_access(
        &self,
        selection: MappingRange,
        access: Access,
    ) -> Result<(), NestError> {
        let key = self.mapping_key(selection.mapping());
        let result = if access.is_leaf_set() {
            self.protect(selection, mapping::Protection::SetAccess(access))
        } else {
            Err(NestError::NotAligned)
        };
        sign::signal(key);
        result
    }

    /// 收窄选择窗口的权限上限与当前权限；架构属性不变。
    pub(crate) fn narrow(&self, selection: MappingRange, ceiling: Access) -> Result<(), NestError> {
        let key = self.mapping_key(selection.mapping());
        let result = self.protect(selection, mapping::Protection::Narrow(ceiling));
        sign::signal(key);
        result
    }

    /// 纯决策在同一 Nest / Mapping 锁内消费，不允许准备与提交间出现过期状态。
    fn protect(
        &self,
        selection: MappingRange,
        request: mapping::Protection,
    ) -> Result<(), NestError> {
        let mapping = selection.mapping().clone();
        let mut state = self.ready_state()?;
        self.ensure_owned(&mapping, &state)?;
        state.protect(self.geometry, &mapping, selection.window(), request)?;
        drop(state);
        self.finish_invalidation()
    }

    fn mapping_key(&self, mapping: &Mapping) -> WakeKey {
        WakeKey::Mapping {
            space: self.id,
            mapping: mapping.identity(),
        }
    }

    pub(crate) fn detach(self: &Arc<Self>, selection: MappingRange) -> Result<(), NestError> {
        let key = self.mapping_key(selection.mapping());
        let result = self.detach_range(selection);
        sign::signal(key);
        result
    }

    /// 按选择撤下：先退休该段占用，再撤 PTE，同步成功后只回收这一段。
    ///
    /// 失败时退休票、Page、表页、来源与待撤的活动全都保留，VA 禁复用；
    /// 沿用同一身份重试（再次 `detach`，或 `retry_invalidation`）才继续。
    fn detach_range(self: &Arc<Self>, selection: MappingRange) -> Result<(), NestError> {
        let mapping = selection.mapping().clone();
        self.check_owner(&mapping.reservation)?;
        if self.is_closed() {
            return Err(NestError::NoRegion);
        }
        if self.has_pending_invalidation() {
            // 先把上一次失败的同步结算掉，再动这一段。
            self.finish_invalidation()?;
        }
        for (start, len) in self.owed_spans(&selection)? {
            self.withdraw(&mapping, start, len)?;
        }
        Ok(())
    }

    /// 选择在当前状态下还欠着的段（活动 ∪ 未结算退休），按地址升序。
    ///
    /// `Pages` 还要求整个窗口都被欠着：选到已经结算完的页就是失败。
    /// 整段只取还欠着的那几段，因此部分撤下之后不会去碰已经没了的洞。
    fn owed_spans(&self, selection: &MappingRange) -> Result<Vec<(VirtAddr, usize)>, NestError> {
        let mapping = selection.mapping();
        let (lo, hi) = selection.window();
        let strict = matches!(selection, MappingRange::Pages { .. });
        let mut owned: Vec<(usize, usize)> = Vec::new();
        {
            let inner = mapping.state.lock();
            owned
                .try_reserve(inner.active.len() + 4)
                .map_err(|_| NestError::OutOfMemory)?;
            for range in &inner.active {
                if let Some((at, last)) = range.intersection(lo, hi) {
                    owned.push((at, last));
                }
            }
        }
        {
            let layout = self.layout.lock();
            for (start, last, _) in layout.occupancy.retired_of(mapping.reservation.id())? {
                let at = start.max(lo);
                let last = last.min(hi);
                if at <= last {
                    owned.push((at, last));
                }
            }
        }
        owned.sort_unstable_by_key(|(start, _)| *start);
        let mut spans = Vec::new();
        spans
            .try_reserve(owned.len())
            .map_err(|_| NestError::OutOfMemory)?;
        let mut floor = lo;
        let mut covered = false;
        for (start, last) in owned {
            if last < floor {
                continue;
            }
            if start > floor {
                if strict {
                    return Err(NestError::NoRegion);
                }
                floor = start;
            }
            spans.push((crate::metal::mmu::canonical(floor), last - floor + 1));
            if last >= hi {
                covered = true;
                break;
            }
            floor = last + 1;
        }
        if strict && !covered {
            return Err(NestError::NoRegion);
        }
        Ok(spans)
    }

    /// 撤下一段：退休该段占用 → 撤 PTE → 同步 → 只回收这一段。
    fn withdraw(
        self: &Arc<Self>,
        mapping: &Arc<Mapping>,
        start: VirtAddr,
        len: usize,
    ) -> Result<(), NestError> {
        if len == 0 || start.offset() != 0 || !len.is_multiple_of(PAGE_SIZE) {
            return Err(NestError::NotAligned);
        }
        let last = start
            .as_usize()
            .checked_add(len - 1)
            .ok_or(NestError::NotAligned)?;
        drop(self.ready_state()?);

        // 地址 → Nest → Mapping：准备与提交共享临界区，不暴露无人接管的退休票。
        let mut layout = self.layout.lock();
        let pieces = layout
            .occupancy
            .pieces(start.as_usize(), last, mapping.reservation.id())?;
        let fresh_count = pieces.iter().filter(|(_, _, ticket)| *ticket == 0).count();
        layout
            .occupancy
            .reserve_cuts(fresh_count.checked_mul(2).ok_or(NestError::OutOfMemory)?)?;
        let mut tickets: Vec<usize> = Vec::new();
        tickets
            .try_reserve(pieces.len())
            .map_err(|_| NestError::OutOfMemory)?;
        let mut state = self.state.lock();
        if state.phase.is_closed() {
            return Err(NestError::NoRegion);
        }
        if !state.retirement.invalidation().is_clean() {
            return Err(NestError::Shootdown);
        }
        self.ensure_owned(mapping, &state)?;
        let mut inner = mapping.state.lock();
        let next = ActiveRange::split(&inner.active, start.as_usize(), last, None)?;
        let first_page = mapping.page_index(start);
        let last_page = mapping.page_index(crate::metal::mmu::canonical(last));
        let mut removed = 0;
        for (page, _) in &inner.published {
            if *page >= first_page && *page <= last_page {
                removed += 1;
            }
        }
        let next_node = if next.is_empty() {
            None
        } else {
            Some(
                Box::try_new(IndexNode::with_ranges(mapping.clone(), &next)?)
                    .map_err(|_| NestError::OutOfMemory)?,
            )
        };
        state
            .retirement
            .cells
            .try_reserve(removed)
            .map_err(|_| NestError::OutOfMemory)?;
        state
            .retirement
            .mappings
            .try_reserve(1)
            .map_err(|_| NestError::OutOfMemory)?;
        state
            .retirement
            .tickets
            .reserve(pieces.len())
            .map_err(|_| NestError::OutOfMemory)?;
        state.prepare_withdrawal(start, len, self.geometry)?;

        // 开票之后没有可失败准备：票、PTE 与关系在同一提交内转入退休责任。
        for (lo, hi, ticket) in pieces {
            tickets.push(if ticket == 0 {
                layout.occupancy.retire(lo, hi, mapping.reservation.id())?
            } else {
                ticket
            });
        }
        state.withdraw_prepared(start, len, self.geometry);
        state.retirement.mappings.push(mapping.clone());
        for index in (0..inner.published.len()).rev() {
            let page = inner.published[index].0;
            if page >= first_page && page <= last_page {
                let (_, cell) = inner.published.remove(index);
                state.retirement.cells.push(cell.retire());
            }
        }
        inner.active = next;
        drop(inner);
        state.index.remove(mapping);
        if let Some(node) = next_node {
            state.index.insert(node);
        }
        for &ticket in &tickets {
            state.retirement.tickets.retire(ticket);
        }
        state.retirement.mark_dirty(mapping.is_global());
        drop(state);
        drop(layout);
        // 三、同步成功之后才结算这一段的票；失败时票、Page、表页与来源都留着。
        self.finish_invalidation()?;
        if !tickets.is_empty() {
            let mut layout = self.layout.lock();
            let mut state = self.state.lock();
            for ticket in tickets {
                if state.retirement.tickets.claim(ticket) {
                    layout.occupancy.reclaim(ticket);
                }
            }
        }
        Ok(())
    }

    /// 接管一笔已落地关系的回收：消费式入口。
    ///
    /// 失败时责任留在 Nest 自己那里：退休票、退休资源与待撤的活动都还在，
    /// 由 `retry_invalidation` 继续；调用者不再持有这笔关系的凭证。
    pub(crate) fn dispose(self: &Arc<Self>, mapping: Arc<Mapping>) -> Result<(), NestError> {
        let key = self.mapping_key(&mapping);
        let result = (|| {
            self.check_owner(&mapping.reservation)?;
            let cleanup = mapping.take_cleanup();
            let result = self.detach_range(mapping.whole());
            if result.is_err()
                && let Some(mut node) = cleanup
            {
                node.mapping = Some(mapping);
                self.enqueue_cleanup(node);
            }
            result
        })();
        sign::signal(key);
        result
    }

    /// 把一段占用对半分成两笔；失败时归还原占用。
    pub(crate) fn split_reservation(
        self: &Arc<Self>,
        reservation: Reservation,
        left_size: usize,
    ) -> Result<(Reservation, Reservation), NestError> {
        self.check_reservation(&reservation)?;
        let split = self.layout.lock().occupancy.split(
            reservation.start().as_usize(),
            reservation.last(),
            reservation.id(),
            left_size,
        );
        let (right_start, left_id, right_id) = match split {
            Ok(split) => split,
            Err(error) => {
                self.give_back(&reservation);
                return Err(error.into());
            }
        };
        let nest = Arc::downgrade(self);
        let region = reservation.region();
        let start = reservation.start();
        let size = reservation.size();
        Ok((
            Reservation::new(nest.clone(), region, start, left_size, left_id),
            Reservation::new(
                nest,
                region,
                crate::metal::mmu::canonical(right_start),
                size - left_size,
                right_id,
            ),
        ))
    }

    /// 交还一笔尚未落地的保留。
    pub(crate) fn release_reservation(&self, reservation: Reservation) -> Result<(), NestError> {
        self.release_reserved(&reservation)
    }

    pub(crate) fn release_reserved(&self, reservation: &Reservation) -> Result<(), NestError> {
        self.check_reservation(reservation)?;
        if self.overlaps(reservation.start(), reservation.size())? {
            return Err(NestError::AlreadyMapped);
        }
        if self.give_back(reservation) {
            Ok(())
        } else {
            Err(NestError::NoRegion)
        }
    }

    fn give_back(&self, reservation: &Reservation) -> bool {
        self.give_back_span(reservation.start(), reservation.size(), reservation.id())
    }

    /// 按占用身份交还一段尚未落地的跨度（凭证已经不在手里时用）。
    fn give_back_span(&self, start: VirtAddr, size: usize, identity: usize) -> bool {
        self.layout.lock().occupancy.release(
            start.as_usize(),
            start.as_usize() + (size - 1),
            identity,
        )
    }

    /// 结算待失效与退休，再收尾清理链上已经消费的关系。
    ///
    /// 这是显式的“原身份重试”入口：它把还欠着的占用票一并交还，然后把已消费的
    /// 关系收到手。正常关闭别的 Mapping 不会走到这里，因此也不会错误结清失败的那段。
    pub(crate) fn retry_invalidation(self: &Arc<Self>) -> Result<(), NestError> {
        self.finish_invalidation()?;
        if self.is_closed() {
            self.drain_closed();
            return Ok(());
        }
        {
            let mut layout = self.layout.lock();
            let mut state = self.state.lock();
            for ticket in state.retirement.tickets.drain() {
                layout.occupancy.reclaim(ticket);
            }
        }
        while let Some(node) = self.take_cleanup() {
            let Some(mapping) = node.mapping.clone() else {
                continue;
            };
            // 已消费的关系只剩收尾：PTE、退休票与退休资源都已清掉时这里是幂等的。
            if let Err(error) = self.detach_range(mapping.whole()) {
                self.enqueue_cleanup(node);
                if self.is_closed() {
                    self.drain_closed();
                    return Ok(());
                }
                return Err(error.into());
            }
        }
        Ok(())
    }

    fn finish_invalidation(&self) -> Result<(), NestError> {
        loop {
            let mut state = self.state.lock();
            if state.retirement.invalidation().is_clean() {
                return Ok(());
            }
            if state.retirement.invalidation().is_fencing() {
                drop(state);
                while self.state.lock().retirement.invalidation().is_fencing() {
                    core::hint::spin_loop();
                }
                continue;
            }
            let target = state.retirement.begin(self.asid);
            drop(state);

            let result = mmu::fence::invalidate(&identity::Cpu, target);
            let fenced = {
                let mut state = self.state.lock();
                match result {
                    Ok(proof) => state.settle(self.asid, proof),
                    Err(_) => {
                        state.retirement.fail();
                        return Err(NestError::Shootdown);
                    }
                }
            };
            // Retired Page and table resources can leave the lock. Address
            // tickets are reclaimed by detach or explicit retry_invalidation.
            fenced.release();
            return Ok(());
        }
    }

    /// 撤下全部关系并等算 shootdown；内核地址空间为进程期，不在此列。
    fn close(&self) -> Result<(), NestError> {
        if self.asid.is_kernel() {
            return Err(NestError::Pinned);
        }
        let mut state = loop {
            let state = self.state.lock();
            if state.phase.is_closed() {
                drop(state);
                self.drain_closed();
                return Ok(());
            }
            if state.retirement.invalidation().is_clean() {
                break state;
            }
            drop(state);
            self.finish_invalidation()?;
        };
        state.close_mappings(self.geometry)?;
        drop(state);
        self.finish_invalidation()?;
        self.drain_closed();
        Ok(())
    }

    /// Close translations, clear waits for this ASID, and settle control bindings
    /// before the identifier can be reused. A failed release keeps it for retry.
    pub(crate) fn retire(&self) -> Result<(), NestError> {
        if !self.asid_released.load(Ordering::Acquire) {
            self.close()?;
            sign::wipe_space(self.id);
            self.reap_mappings();
            self.release_asid()?;
        }
        self.reap_mappings();
        Ok(())
    }

    fn release_asid(&self) -> Result<(), NestError> {
        let _state = self.state.lock();
        if !self.asid_released.load(Ordering::Acquire) {
            identity::deallocate(self.asid).map_err(|_| NestError::Shootdown)?;
            self.asid_released.store(true, Ordering::Release);
        }
        Ok(())
    }

    fn enqueue_cleanup(&self, mut node: Box<CleanupNode>) {
        let mut pending = self.pending_cleanup.lock();
        if !self.is_closed() {
            node.next = pending.take();
            *pending = Some(node);
            return;
        }
        drop(pending);
        drop(node);
    }

    /// 与晚到入链共用串行点；关系析构必须在地址占用锁与 Nest 状态锁外。
    fn drain_closed(&self) {
        let mut nodes = {
            let mut pending = self.pending_cleanup.lock();
            if !self.is_closed() {
                return;
            }
            pending.take()
        };
        while let Some(mut node) = nodes {
            nodes = node.next.take();
            drop(node);
        }
    }

    /// 取一环待清理的消费式任务；处理完不再放回。
    fn take_cleanup(&self) -> Option<Box<CleanupNode>> {
        let mut pending = self.pending_cleanup.lock();
        let mut node = pending.take()?;
        *pending = node.next.take();
        Some(node)
    }

    /// 选择的所属与阶段：认住它的那个 Nest，且没有关闭。
    fn ensure_owned(&self, mapping: &Arc<Mapping>, state: &NestState) -> Result<(), NestError> {
        let owner = mapping.reservation.nest().ok_or(NestError::NoRegion)?;
        if state.phase.is_closed()
            || !core::ptr::eq(Arc::as_ptr(&owner) as *const Nest, self as *const Nest)
        {
            return Err(NestError::NoRegion);
        }
        Ok(())
    }

    fn check_owner(&self, reservation: &Reservation) -> Result<(), NestError> {
        let owner = reservation.nest().ok_or(NestError::NoRegion)?;
        if !core::ptr::eq(Arc::as_ptr(&owner) as *const Nest, self as *const Nest) {
            return Err(NestError::NoRegion);
        }
        Ok(())
    }

    fn check_reservation(&self, reservation: &Reservation) -> Result<(), NestError> {
        self.check_owner(reservation)?;
        if !self.layout.lock().occupancy.has_id(reservation.id()) {
            return Err(NestError::NoRegion);
        }
        Ok(())
    }

    fn is_closed(&self) -> bool {
        self.state.lock().phase.is_closed()
    }

    /// 翻译里只取物理地址的那一面：权限位不外露。
    pub(crate) fn physical_at(&self, address: VirtAddr) -> Option<PhysAddr> {
        self.translate(address).map(|(physical, _)| physical)
    }

    pub(crate) fn translate(&self, address: VirtAddr) -> Option<(PhysAddr, Attributes)> {
        let state = self.ready_state().ok()?;
        state.index.find(address.as_usize())?;
        let (pa, flags) = state.root.walk_ref_geometry(address, self.geometry).ok()?;
        Some((pa + address.offset(), flags))
    }

    /// 用户页错误的纯一致探针：只读当前的身份、活动权限、承载与硬件条目。
    ///
    /// 直取 `Nest → Mapping → Flat` 三层状态，不推进失效、不物化、不分配、不新造
    /// 页状态，因此可以在 work 的提交边界内作为条件等待的谓词跑一次。
    /// 待结算的失效先归到 `Materialize`：那份失效一旦真的未确认，旧 PTE 就不是
    /// “已经可用”的证据，也不能据此把一个已经退场的关系判死。
    /// 承载先于 PTE 看：`Fresh`/`Resident`/有效借用再看条目，满足才是 `Resolved`；
    /// `Missing`/`Moving`/`Reserved` 一律 `Wait`，不因旧 PTE 还在就绕过等待；
    /// 身份消失或活动权限不容纳这次访问是 `Invalid`。
    pub(crate) fn probe_fault(&self, address: VirtAddr, kind: FaultKind) -> FaultOutcome {
        FaultOutcome::probe(&self.state.lock(), self.geometry, address, kind)
    }

    /// 用户页错误的完整解析：先结清待办失效，再按纯探针的结论物化并发布，
    /// 最后重新纯探一次交回结论。全程在 work 的提交边界之外。
    ///
    /// 与并发移交撞上的竞态（`Busy`/`Missing`/关系已退场）折回一次纯重探，
    /// 交回 `Wait`/`Materialize`/`Invalid`；真实资源失败（分配耗尽、同步失败）
    /// 以 `Err` 交出来，由入口记录后终止任务，不伪装成缺页或权限错误。
    pub(crate) fn resolve_fault(
        self: &Arc<Self>,
        address: VirtAddr,
        kind: FaultKind,
    ) -> Result<FaultOutcome, NestError> {
        if self.has_pending_invalidation() {
            self.retry_invalidation()?;
        }
        if let FaultOutcome::Materialize = self.probe_fault(address, kind) {
            match self.materialize_page(address) {
                Ok(()) => {}
                // 并发移交或关系已退场：重探会给出真实结论，不在这里判死。
                Err(NestError::Busy | NestError::Missing | NestError::NoRegion) => {}
                // 真实资源失败：照实交回，入口记录后终止。
                Err(error) => return Err(error),
            }
        }
        Ok(self.probe_fault(address, kind))
    }

    /// 物化并发布缺页所在的那一页；关系已消失照实报错。
    fn materialize_page(self: &Arc<Self>, address: VirtAddr) -> Result<(), NestError> {
        let page = address.page_align();
        let selection = {
            let state = self.state.lock();
            let mapping = state
                .index
                .find(address.as_usize())
                .ok_or(NestError::NoRegion)?;
            mapping.range(
                PageOffset::new(mapping.page_index(page)),
                PageCount::new(1).expect("one fault page"),
            )?
        };
        self.materialize_active(selection)
    }

    #[cfg(debug_assertions)]
    pub(crate) fn audit(&self) {
        self.state.lock().audit(self, self.geometry);
    }
}

impl Drop for Nest {
    fn drop(&mut self) {
        if !self.published.load(Ordering::Acquire) {
            if !self.asid.is_kernel() {
                Asid::release_unpublished(self.asid);
            }
        } else if self.asid.is_kernel() {
            return;
        } else if !self.asid_released.load(Ordering::Acquire) {
            sign::wipe_space(self.id);
            if self.close().and_then(|()| self.release_asid()).is_err() {
                return;
            }
        }
        unsafe { ManuallyDrop::drop(&mut self.state) };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpaceKind {
    Supervisor,
    User,
}

impl SpaceKind {
    pub fn is_supervisor(self) -> bool {
        matches!(self, SpaceKind::Supervisor)
    }
}

impl From<kernel_api::model::unit::ProgramKind> for SpaceKind {
    fn from(k: kernel_api::model::unit::ProgramKind) -> Self {
        match k {
            kernel_api::model::unit::ProgramKind::User => SpaceKind::User,
            kernel_api::model::unit::ProgramKind::Supervisor => SpaceKind::Supervisor,
        }
    }
}
