//! 地址翻译：地址占用、翻译关系、硬件后端、身份与同步退休。
//!
//! Flat 继续独立管理来源与物化；这里不保存来源页槽。

mod access;
pub(crate) mod address;
pub(crate) mod authority;
mod error;
pub(crate) mod index;
pub(crate) mod memo;
mod translation;
pub(crate) mod wait;

use alloc::boxed::Box;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::mem::ManuallyDrop;

use crate::basis::cell::page::{Change, Freeze, Page};
use crate::basis::flat::Pages;
use crate::basis::sign::{self, WakeKey};
use crate::basis::unit::anchor::Anchor;
use crate::metal::generic::mmu::{Attributes, Domain, PageTable, fence};
use crate::metal::layout::TEAM_FRAME_BASE;
use crate::metal::layout::TEAM_FRAME_WINDOW_SIZE;
use crate::metal::layout::TRAP_STACK_BASE;
use crate::metal::mmu::{self, Geometry, Root, Table, identity};
use crate::metal::sync::Level;
use crate::metal::sync::SpinLock;
use crate::metal::sync::SpinLockGuard;
use kernel_api::address::PAGE_SIZE;
use kernel_api::address::PageCount;
use kernel_api::address::PageOffset;
use kernel_api::address::PhysAddr;
use kernel_api::address::VirtAddr;
use translation::Invalidation;

use crate::metal::generic::trap::FaultKind;
pub use address::Reservation;
pub(crate) use address::ReservedMapping;
pub use address::span::{Access, ActiveRange, Span};
pub use error::NestError;
pub use identity::Asid;
pub use translation::FaultOutcome;

use address::Layout;
pub(crate) use address::NestRange;
pub(crate) use address::{Placement, PlacementAt, RegionKind};

use index::IndexNode;
use index::source;
pub(crate) use memo::publication::Error as PublicationError;
use memo::publication::Publication;

use translation::NestState;

/// 待清理链条上的一环：一笔已经消费、但还没收尾的关系。
///
/// 节点在发布前预留下来，接管回收时不再分配；
/// 它只保存关系的强持，占用票（退休）与 Page / 表页的账在 Nest 自己那里。
struct CleanupNode {
    mapping: Option<Arc<Span>>,
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

/// 一个地址组织：身份、翻译后端、全局占用、关系与同步退休。
///
/// Nest owns each Memo and its publication. Address spans describe source
/// selection and permissions; the index never duplicates per-position state.
pub use kernel_api::model::nest::NestId;
fn next_id() -> Result<NestId, NestError> {
    static NEXT: crate::basis::name::Serial =
        crate::basis::name::Serial::new(1, isize::MAX as usize);
    NEXT.allocate()
        .and_then(NestId::new)
        .ok_or(NestError::OutOfMemory)
}

struct CpuContext {
    domain: mmu::Domain,
    kind: SpaceKind,
}

pub struct Nest {
    id: NestId,
    cpu: Option<CpuContext>,
    life: Arc<Anchor>,
    pub(crate) mappings: authority::RangeRegistry,
    layout: SpinLock<Layout>,
    pending_cleanup: SpinLock<Option<Box<CleanupNode>>>,
    state: ManuallyDrop<SpinLock<NestState>>,
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
            let dram = crate::entry::boot::facts::info().memory.envelope();
            let base = mmu::lower()
                .as_usize()
                .checked_add(dram.base)
                .and_then(|base| base.checked_add(dram.size))
                .ok_or(NestError::DramOverlap)?;
            layout.register_window(RegionKind::Resource, base, TRAP_STACK_BASE.as_usize() - 1)?;
        }
        let life = Anchor::try_new()?;
        let id = next_id()?;
        let domain = mmu::Domain::create(id.get(), kernel)?;
        let nest = Arc::try_new(Self {
            id,
            cpu: Some(CpuContext { domain, kind }),
            life,
            mappings: authority::RangeRegistry::new(),
            layout: SpinLock::new_level(Level::Address, layout),
            pending_cleanup: SpinLock::new_level(Level::Address, None),
            state: ManuallyDrop::new(SpinLock::new_level(Level::Nest, NestState::new(root))),
        })
        .map_err(|_| NestError::OutOfMemory)?;
        nest.cpu
            .as_ref()
            .expect("CPU construction")
            .domain
            .publish();
        Ok(nest)
    }

    /// 尚未发布的事务只在这些阶段可见：Clean 且未关闭。
    fn ready_state(&self) -> Result<SpinLockGuard<'_, NestState>, NestError> {
        loop {
            let state = self.state.lock();
            if !state.phase.is_open() {
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
        self.cpu.as_ref().expect("executable Nest").kind
    }

    pub(crate) fn life(&self) -> Weak<Anchor> {
        Arc::downgrade(&self.life)
    }

    pub fn id(&self) -> NestId {
        self.id
    }

    pub(crate) fn is_open(&self) -> bool {
        self.state.lock().is_open()
    }

    pub(crate) fn is_adopted(&self) -> bool {
        self.state.lock().team.is_some()
    }

    /// The Team becomes the CPU Nest's lifetime owner exactly once.
    pub(crate) fn attach_team(
        &self,
        team: kernel_api::model::identity::TeamId,
    ) -> Result<(), NestError> {
        if self.is_external() {
            return Err(NestError::NoRegion);
        }
        let mut state = self.state.lock();
        if !state.is_open()
            || state.team.is_some()
            || !state.retirement.invalidation().is_clean()
            || !self.mappings.empty()
        {
            return Err(NestError::Busy);
        }
        state.team = Some(team);
        Ok(())
    }

    pub(crate) fn mapping(&self, id: kernel_api::model::cell::MappingId) -> Option<Arc<Span>> {
        self.mappings.registered(id.get())?.mapping
    }

    pub(crate) fn mapping_open(&self, id: kernel_api::model::cell::MappingId) -> bool {
        self.is_open() && self.mapping(id).is_some_and(|mapping| mapping.is_open())
    }

    pub(crate) fn mapping_status(
        &self,
        id: kernel_api::model::cell::MappingId,
    ) -> kernel_api::model::pend::PendStatus {
        use address::span::MappingPhase;
        use kernel_api::model::pend::PendStatus;
        match self.mapping(id).map(|mapping| mapping.state.lock().phase) {
            Some(MappingPhase::Open) if self.is_open() => PendStatus::Active,
            Some(MappingPhase::Open | MappingPhase::Closing) => PendStatus::Retiring,
            Some(MappingPhase::Gone) | None => PendStatus::Dead,
        }
    }

    pub(crate) fn mapping_root(
        &self,
        id: kernel_api::model::cell::MappingId,
        root: &Arc<crate::basis::pend::Pend>,
    ) -> bool {
        self.mappings.registered(id.get()).is_some_and(|entry| {
            entry
                .root
                .as_ref()
                .is_some_and(|known| known.authority.ptr_eq(&Arc::downgrade(root)))
        })
    }

    pub(crate) fn mapping_count(
        &self,
        id: kernel_api::model::cell::MappingId,
    ) -> Option<PageCount> {
        PageCount::new(self.mapping(id)?.initial_size() / PAGE_SIZE)
    }

    pub(crate) fn mapping_level(
        &self,
        id: kernel_api::model::cell::MappingId,
    ) -> Option<kernel_api::model::cell::CellLevel> {
        Some(self.mapping(id)?.level())
    }

    pub(crate) fn asid(&self) -> Asid {
        self.cpu
            .as_ref()
            .expect("executable Nest")
            .domain
            .identifier()
    }

    fn geometry(&self) -> Geometry {
        self.cpu
            .as_ref()
            .expect("CPU translation geometry")
            .domain
            .geometry()
    }

    pub(crate) fn root(&self) -> Root {
        Root {
            frame: self
                .state
                .lock()
                .root
                .cpu()
                .expect("execution root requires CPU target")
                .frame(),
            asid: self.asid(),
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

    pub(crate) fn binding_source(
        &self,
        binding: &Arc<authority::RangeBinding>,
    ) -> Option<memo::source::Source> {
        let index = binding.page()?;
        let mapping = self.mappings.registered(binding.id())?.mapping?;
        let state = self.state.lock();
        let memo = state.memos.get(&mapping, index)?;
        if !memo.matches(binding) {
            return None;
        }
        memo.association().map(|(_, source, _)| source)
    }

    pub(crate) fn binding_pending(&self, binding: &Arc<authority::RangeBinding>) -> bool {
        let Some(entry) = self.mappings.registered(binding.id()) else {
            return false;
        };
        let Some(mapping) = entry.mapping() else {
            return binding.page().is_none() && Arc::ptr_eq(&entry.binding, binding);
        };
        let state = self.state.lock();
        if let Some(index) = binding.page() {
            return state
                .memos
                .get(mapping, index)
                .is_some_and(|memo| memo.matches(binding) && memo.busy());
        }
        let borrowed = {
            let inner = mapping.state.lock();
            !inner.active.is_empty()
                && inner
                    .source
                    .as_ref()
                    .is_some_and(|source| source.is_borrowed())
        };
        Arc::ptr_eq(&entry.binding, binding)
            && !state.phase.is_closed()
            && (!state.retirement.invalidation().is_clean()
                || state.memos.published(mapping).next().is_some()
                || borrowed)
    }

    /// Close reservation admission before contending for its operation gate.
    pub(crate) fn release_mapping(
        &self,
        id: kernel_api::model::cell::MappingId,
    ) -> Result<(), kernel_api::abi::call::cell::CellFail> {
        use address::span::MappingPhase;
        use kernel_api::abi::call::cell::CellFail;
        let Some(mapping) = self.mapping(id) else {
            return Ok(());
        };
        {
            let _commit = crate::basis::commit();
            mapping.close();
        }
        let _operation = mapping.operation().ok_or(CellFail::Busy)?;
        if mapping.state.lock().phase == MappingPhase::Gone {
            return Ok(());
        }
        if mapping.source().is_some() {
            return Err(CellFail::Denied);
        }
        if !self.is_external() {
            self.finish_invalidation()?;
        }
        let mut cursor = None;
        let mut busy = false;
        loop {
            let index = self.state.lock().memos.next_page(&mapping, cursor);
            let Some(index) = index else { break };
            cursor = Some(index);
            self.finish_memo(&mapping, index);
            let pending = self
                .state
                .lock()
                .memos
                .get(&mapping, index)
                .is_some_and(memo::Memo::busy);
            if pending {
                let Some(page) = self.memo_page(&mapping, index)? else {
                    busy = true;
                    continue;
                };
                let change = match crate::basis::cell::change::acquire(&page) {
                    Ok(change) => change,
                    Err(_) => {
                        busy = true;
                        continue;
                    }
                };
                if self
                    .retract_position(&mapping, index, None, change, true)
                    .is_err()
                {
                    busy = true;
                    continue;
                }
            }
            let cleared = {
                let _commit = crate::basis::commit();
                self.state
                    .lock()
                    .memos
                    .existing_mut(&mapping, index)
                    .expect("selected Memo")
                    .clear()
            };
            match cleared {
                Ok(cleared) => cleared.release(),
                Err(_) => busy = true,
            }
        }
        if busy {
            return Err(CellFail::Busy);
        }
        {
            let _commit = crate::basis::commit();
            let mut layout = self.layout.lock();
            let mut state = self.state.lock();
            if !state.retirement.invalidation().is_clean() || state.memos.has_resources(&mapping) {
                return Err(CellFail::Busy);
            }
            state.index.remove(&mapping);
            layout.occupancy.release_settled(mapping.identity());
            let mut inner = mapping.state.lock();
            inner.active.clear();
            inner.phase = MappingPhase::Gone;
        }
        sign::signal(self.mapping_key(&mapping));
        if self.is_external() {
            sign::signal(WakeKey::ExternalMapping { id: id.get() });
        }
        Ok(())
    }

    pub(crate) fn withdraw_mapping(
        self: &Arc<Self>,
        id: kernel_api::model::cell::MappingId,
        window: (PageOffset, PageCount),
        authority: &Arc<crate::basis::pend::Pend>,
    ) -> Result<(), kernel_api::abi::call::pend::PendFail> {
        use kernel_api::abi::call::pend::PendFail;
        use kernel_api::model::pend::{NestScope, PendScope};
        let Some(mapping) = self.mapping(id) else {
            return Ok(());
        };
        let _operation = mapping.operation().ok_or(PendFail::Busy)?;
        mapping
            .range(window.0, window.1)
            .map_err(|_| PendFail::NotAligned)?;
        if !matches!(authority.scope(), PendScope::Nest(NestScope::Mapping { id: known, .. }) if known == id)
        {
            return Err(PendFail::Denied);
        }
        if !self.is_external() {
            self.finish_invalidation().map_err(|_| PendFail::Busy)?;
        }
        let end = window
            .0
            .get()
            .checked_add(window.1.get())
            .ok_or(PendFail::NotAligned)?;
        let mut cursor = None;
        let mut busy = false;
        loop {
            let Some(index) = self.state.lock().memos.next_page(&mapping, cursor) else {
                break;
            };
            cursor = Some(index);
            if index < window.0.get() || index >= end {
                continue;
            }
            self.finish_memo(&mapping, index);
            if !self.memo_published_by(&mapping, index, authority) {
                continue;
            }
            let Some(page) = self
                .memo_page(&mapping, index)
                .map_err(|_| PendFail::Busy)?
            else {
                busy = true;
                continue;
            };
            match crate::basis::cell::change::acquire(&page) {
                Ok(change) => {
                    busy |= self
                        .retract_position(&mapping, index, None, change, true)
                        .is_err()
                }
                Err(_) => busy = true,
            }
        }
        sign::signal(self.mapping_key(&mapping));
        if self.is_external() {
            sign::signal(WakeKey::ExternalMapping { id: id.get() });
        }
        if busy { Err(PendFail::Busy) } else { Ok(()) }
    }

    pub(crate) fn withdraw_source(
        self: &Arc<Self>,
        binding: &Arc<authority::RangeBinding>,
        authorization: &Arc<crate::basis::pend::SourceAuthorization>,
        token: Option<kernel_api::model::identity::PendId>,
        actor: Option<kernel_api::model::identity::TaskId>,
        clear: bool,
    ) -> Result<(), kernel_api::abi::call::pend::PendFail> {
        use kernel_api::abi::call::pend::PendFail;
        let Some(entry) = self.mappings.registered(binding.id()) else {
            return Ok(());
        };
        let Some(mapping) = entry.mapping() else {
            return Err(PendFail::Busy);
        };
        let _operation = mapping.operation().ok_or(PendFail::Busy)?;
        let Some(index) = binding.page() else {
            let selected = {
                let _commit = crate::basis::commit();
                Arc::ptr_eq(&entry.binding, binding)
                    && entry
                        .source()
                        .is_some_and(|source| Arc::ptr_eq(source, authorization))
            };
            if !selected || token.is_some() || actor.is_some() {
                return Ok(());
            }
            if clear {
                self.dispose(mapping.clone()).map_err(|_| PendFail::Busy)?;
                self.retire_mapping(binding.id());
                return Ok(());
            }
            if mapping.source().is_some_and(|source| source.is_borrowed()) {
                return Err(PendFail::Busy);
            }
            let mut cursor = None;
            loop {
                let Some(index) = self.state.lock().memos.next_page(mapping, cursor) else {
                    break;
                };
                cursor = Some(index);
                let Some(page) = self.memo_page(mapping, index).map_err(|_| PendFail::Busy)? else {
                    continue;
                };
                self.unpublish_page(
                    mapping,
                    VirtAddr::new(mapping.position(index).map_err(|_| PendFail::Busy)?),
                    &page,
                )
                .map_err(|_| PendFail::Busy)?;
                self.finish_memo(mapping, index);
            }
            return Ok(());
        };
        let selected = |source: &memo::source::Source| {
            Arc::ptr_eq(source.source(), authorization)
                && token.is_none_or(|token| source.matches_branch(token))
                && actor.is_none_or(|actor| source.belongs_to(actor))
        };
        let page = {
            let _commit = crate::basis::commit();
            let state = self.state.lock();
            let Some(memo) = state.memos.get(mapping, index) else {
                return Ok(());
            };
            if !memo.matches(binding) {
                return Ok(());
            }
            let Some((_, source, _)) = memo.association() else {
                return Ok(());
            };
            let page = memo.page();
            drop(state);
            if !selected(&source) {
                return Ok(());
            }
            page
        };
        if !self.is_external() {
            self.finish_invalidation().map_err(|_| PendFail::Busy)?;
        }
        self.finish_memo(mapping, index);
        let pending = self
            .state
            .lock()
            .memos
            .get(mapping, index)
            .is_some_and(memo::Memo::busy);
        if pending {
            let page = page.ok_or(PendFail::Busy)?;
            let change = crate::basis::cell::change::acquire(&page).map_err(|_| PendFail::Busy)?;
            self.retract_position(mapping, index, None, change, true)
                .map_err(|_| PendFail::Busy)?;
        }
        if clear {
            let cleared = {
                let _commit = crate::basis::commit();
                // Revalidate the exact shared binding; a stale (id,page) cannot clear a replacement.
                let mut state = self.state.lock();
                let Some(memo) = state.memos.existing_mut(mapping, index) else {
                    return Ok(());
                };
                if !memo.matches(binding) {
                    return Ok(());
                }
                memo.clear().map_err(|_| PendFail::Busy)?
            };
            cleared.release();
        }
        sign::signal(self.mapping_key(mapping));
        if self.is_external() {
            sign::signal(WakeKey::ExternalMapping {
                id: mapping.identity(),
            });
        }
        Ok(())
    }

    /// Prepare a source-free namespace transaction; only publish makes it visible.
    pub(crate) fn reserve_mapping(
        self: &Arc<Self>,
        placement: Placement,
        level: kernel_api::model::cell::CellLevel,
        access: Access,
        creator: kernel_api::model::identity::TaskId,
    ) -> Result<ReservedMapping, NestError> {
        self.reap_mappings();
        let cell_size = level.bytes().ok_or(NestError::NotAligned)?;
        if !access.is_leaf_set() || !placement.size().is_multiple_of(cell_size) {
            return Err(NestError::NotAligned);
        }
        if self.is_external() {
            let PlacementAt::Exact(address) = placement.at() else {
                return Err(NestError::NotAligned);
            };
            if level.get() > 1
                || placement.size() != cell_size
                || address.get() == 0
                || address.get() >= 1usize << 38
                || !address.get().is_multiple_of(cell_size)
                || access.intersects(Access::EXECUTE)
                || !access.contains(Access::READ)
            {
                return Err(NestError::NotAligned);
            }
            if self.mappings.competing_reservation(creator) {
                return Err(NestError::Busy);
            }
        } else if level.get() >= crate::metal::mmu::mode::levels() {
            return Err(NestError::NotAligned);
        }
        if !self.is_open() {
            return Err(NestError::NoRegion);
        }
        let reservation = self.reserve(placement)?;
        ReservedMapping::new(self, reservation, level, access, creator)
    }

    /// 普通挂载的架构属性：用户空间只对 `Normal` 区开 U，A/D 由内核自维护。
    pub(crate) fn attrs(&self, region: Option<RegionKind>) -> Attributes {
        if self.cpu.is_none() {
            return Attributes::empty();
        }
        let mut attrs = Attributes::ACCESSED | Attributes::DIRTY;
        if !self.kind().is_supervisor() && region == Some(RegionKind::Normal) {
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
            if self.cpu.is_some() {
                crate::metal::mmu::canonical(start)
            } else {
                VirtAddr::new(start)
            },
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
    ) -> Result<Arc<Span>, NestError> {
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
    ) -> Result<Arc<Span>, NestError> {
        if let Err(error) = self.check_owner(&reservation) {
            return Err(Self::abandon(reservation, error));
        }
        if !Span::attrs_are_architectural(attrs) {
            return Err(Self::abandon(reservation, NestError::NotAligned));
        }
        let mut ranges = Vec::new();
        if ranges.try_reserve(active.len()).is_err() {
            return Err(Self::abandon(reservation, NestError::OutOfMemory));
        }
        ranges.extend_from_slice(active);
        let level = source.level();
        let mapping = Span::new(reservation, attrs, ranges, level, Some(source))?;
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
    fn settle_failed_install(&self, mapping: &Arc<Span>) {
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
    fn commit(self: &Arc<Self>, mapping: &Arc<Span>) -> Result<(), NestError> {
        let implicit = mapping.source().ok_or(NestError::Missing)?;
        if self.cpu.is_none() {
            return Err(NestError::NoRegion);
        }
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
        let lease = implicit.reside()?;
        {
            let inner = mapping.state.lock();
            if implicit.is_borrowed() {
                pages = Some(implicit.materialize()?);
                borrowed
                    .try_reserve(inner.active.len())
                    .map_err(|_| NestError::OutOfMemory)?;
                for range in &inner.active {
                    borrowed.push((range.start, range.size.get(), range.current));
                }
            } else {
                let materialized = implicit.materialized_cells()?;
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
        state.memos.reserve(published.len())?;
        // 首条 PTE 之前登记来源↔关系：撤页要按来源枚举全部关系，不能只看用户注册表。
        source::register(&implicit, mapping, None)?;
        for run in mapping.state.lock().runs(start, &published) {
            let run = run?;
            state.root.cpu_mut()?.prepare_range_geometry(
                run.start,
                run.physical,
                run.size,
                self.geometry(),
            )?;
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
                .cpu_mut()?
                .prepare_range_geometry(*at, pa, *len, self.geometry())?;
        }
        // 可失败准备到此为止：这条关系随后只进不可失败的 PTE 提交，
        // 而短期驻留仍守着这份来源。内核地址空间与全局条目都让整份来源
        // 永久失去迁移资格。
        if self.asid().is_kernel() || mapping.is_global() {
            implicit.mark_untransferable();
        }
        for (_, retained) in &mut published {
            retained.publish();
        }
        state.retirement.mappings.push(mapping.clone());
        for run in mapping.state.lock().runs(start, &published) {
            let run = run?;
            state.root.cpu_mut()?.map_range_prepared_geometry(
                run.start,
                run.physical,
                run.size,
                mapping.leaf_attributes(run.current),
                self.geometry(),
            );
        }
        for (at, len, current) in &borrowed {
            let pa = pages
                .as_ref()
                .expect("borrowed runs imply a borrowed source")
                .physical(PageOffset::new(
                    (at.as_usize() - start.as_usize()) / PAGE_SIZE,
                ))?;
            state.root.cpu_mut()?.map_range_prepared_geometry(
                *at,
                pa,
                *len,
                mapping.leaf_attributes(*current),
                self.geometry(),
            );
        }
        for (page, cell) in published {
            state.memos.publish(mapping, page, cell);
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
    fn publish(self: &Arc<Self>, selection: NestRange) -> Result<(), NestError> {
        let mapping = selection.mapping().clone();
        for (start, size) in self.active_spans(&selection)? {
            self.publish_span(&mapping, start, size)?;
        }
        Ok(())
    }

    fn publish_span(
        self: &Arc<Self>,
        mapping: &Arc<Span>,
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
        let inner = mapping.state.lock();
        if !inner.covers(start.as_usize(), end) {
            return Err(NestError::NoRegion);
        }
        let source = inner.source.as_ref().ok_or(NestError::Missing)?;
        if source.is_borrowed() {
            return Ok(());
        }
        let first = mapping.page_index(start);
        let window = source.range(
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
            if state.memos.publication(mapping, page).is_none() {
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
        state.memos.reserve(fresh.len())?;
        state
            .retirement
            .mappings
            .try_reserve(1)
            .map_err(|_| NestError::OutOfMemory)?;
        for run in inner.runs(mapping.initial_start(), &fresh) {
            let run = run?;
            state.root.cpu_mut()?.prepare_range_geometry(
                run.start,
                run.physical,
                run.size,
                self.geometry(),
            )?;
        }
        // 可失败准备到此为止：随后只有这一页的 PTE 提交，短期驻留仍守着这份来源。
        // 内核地址空间与全局条目都让整份来源永久失去迁移资格。
        if self.asid().is_kernel() || mapping.is_global() {
            source.mark_untransferable();
        }
        for (_, retained) in &mut fresh {
            retained.publish();
        }
        state.retirement.mappings.push(mapping.clone());
        for run in inner.runs(mapping.initial_start(), &fresh) {
            let run = run?;
            state.root.cpu_mut()?.map_range_prepared_geometry(
                run.start,
                run.physical,
                run.size,
                mapping.leaf_attributes(run.current),
                self.geometry(),
            );
        }
        for (page, cell) in fresh {
            state.memos.publish(mapping, page, cell);
        }
        state.retirement.mark_dirty(mapping.is_global());
        drop(inner);
        drop(state);
        drop(lease);
        self.finish_invalidation()
    }

    pub(crate) fn memo_page(
        &self,
        mapping: &Arc<Span>,
        page: usize,
    ) -> Result<Option<Arc<Page>>, NestError> {
        self.check_owner(&mapping.reservation)?;
        mapping.position(page)?;
        let state = self.state.lock();
        Ok(state.memos.get(mapping, page).and_then(|memo| memo.page()))
    }

    pub(crate) fn associate(
        &self,
        mapping: &Arc<Span>,
        index: usize,
        expected: u64,
        page: Arc<Page>,
        source: memo::source::Source,
    ) -> Result<(), NestError> {
        self.associate_kind(mapping, index, expected, Some(page), source)
    }

    pub(crate) fn associate_logical(
        &self,
        mapping: &Arc<Span>,
        index: usize,
        expected: u64,
        source: memo::source::Source,
    ) -> Result<(), NestError> {
        if mapping.source().is_some() {
            return Err(NestError::Pinned);
        }
        self.associate_kind(mapping, index, expected, None, source)
    }

    fn associate_kind(
        &self,
        mapping: &Arc<Span>,
        index: usize,
        expected: u64,
        page: Option<Arc<Page>>,
        source: memo::source::Source,
    ) -> Result<(), NestError> {
        let _operation = mapping.operation().ok_or(NestError::Busy)?;
        let _commit = crate::basis::commit();
        self.ensure_mapping(mapping)?;
        mapping.access(index)?;
        if !self.is_open() || !mapping.is_open() {
            return Err(NestError::NoRegion);
        }
        let range = match &page {
            Some(page) => source.same_page(page),
            None => source.range(),
        }
        .map_err(memo::source_error)?;
        if page.as_ref().is_some_and(|p| p.level() != mapping.level())
            || range.level() != mapping.level()
        {
            return Err(NestError::NotAligned);
        }
        let (offset, count) = source.window();
        source
            .source()
            .check_move(offset, count, source.hold().authority().map(Arc::as_ref))
            .map_err(memo::namespace_error)?;
        if page.is_none() {
            source
                .source()
                .check_publish(offset, count)
                .map_err(memo::namespace_error)?;
        }
        let owner = mapping.reservation.nest().ok_or(NestError::NoRegion)?;
        let binding = authority::RangeBinding::new_position(mapping.identity(), &owner, index)
            .map_err(memo::namespace_error)?;
        source
            .source()
            .reserve_binding()
            .map_err(memo::namespace_error)?;
        {
            let mut state = self.state.lock();
            if !state.retirement.invalidation().is_clean() {
                return Err(NestError::Busy);
            }
            let memo = state.memos.get_mut(mapping, index)?;
            if page.is_none() && !memo.accepts_logical() {
                return Err(NestError::Pinned);
            }
            memo.can_associate(expected)?;
        }
        if !self.is_external() {
            index::source::register(&range, mapping, Some(&binding))?;
        }
        if let Some(page) = &page {
            source
                .source()
                .retain_blocked(offset, count, page)
                .map_err(memo::namespace_error)?;
            source.same_page(page).map_err(memo::source_error)?;
        } else {
            source.range().map_err(memo::source_error)?;
        }
        let authorization = source.source().clone();
        let replaced = {
            let mut state = self.state.lock();
            if !state.is_open() || !mapping.is_open() {
                return Err(NestError::NoRegion);
            }
            state
                .memos
                .existing_mut(mapping, index)
                .expect("prepared Memo")
                .associate(expected, page, source, binding.clone())
        };
        authorization.register_binding(binding);
        replaced.release();
        Ok(())
    }

    pub(crate) fn association(
        &self,
        mapping: &Arc<Span>,
        index: usize,
    ) -> Result<Option<(Arc<Page>, memo::source::Source)>, NestError> {
        self.check_owner(&mapping.reservation)?;
        mapping.position(index)?;
        let selected = self
            .state
            .lock()
            .memos
            .get(mapping, index)
            .and_then(memo::Memo::association);
        let Some((page, source, _)) = selected else {
            return Ok(None);
        };
        let page = match page {
            Some(page) => page,
            None => source
                .range()
                .map_err(memo::source_error)?
                .page(PageOffset::ZERO)?
                .ok_or(NestError::Missing)?,
        };
        Ok(Some((page, source)))
    }

    /// Persistent source lookup never materializes or retains a physical Page.
    pub(crate) fn logical_source(
        &self,
        mapping: &Arc<Span>,
        index: usize,
        expected: u64,
    ) -> Result<Option<memo::source::Source>, NestError> {
        self.check_owner(&mapping.reservation)?;
        mapping.position(index)?;
        if mapping.source().is_some() {
            return Err(NestError::Pinned);
        }
        let state = self.state.lock();
        let Some(memo) = state.memos.get(mapping, index) else {
            return if expected == 1 {
                Ok(None)
            } else {
                Err(NestError::Busy)
            };
        };
        if memo.generation() != expected {
            return Err(NestError::Busy);
        }
        if !memo.accepts_logical() {
            return Err(NestError::Pinned);
        }
        Ok(memo.association().map(|(_, source, _)| source))
    }

    pub(crate) fn inspect_logical(
        &self,
        mapping: &Arc<Span>,
        index: usize,
    ) -> Result<kernel_api::model::nest::MemoStatus, NestError> {
        use kernel_api::model::{
            cell::MappingId,
            nest::{MemoGeneration, MemoPhase, MemoStatus},
        };
        self.check_owner(&mapping.reservation)?;
        mapping.position(index)?;
        if mapping.source().is_some() {
            return Err(NestError::Pinned);
        }
        let state = self.state.lock();
        let memo = state.memos.get(mapping, index);
        if memo.is_some_and(|memo| !memo.accepts_logical()) {
            return Err(NestError::Pinned);
        }
        if memo.is_some_and(memo::Memo::busy) {
            return Err(NestError::Busy);
        }
        let source = memo
            .and_then(memo::Memo::association)
            .map(|(_, source, _)| source.position());
        Ok(MemoStatus {
            nest: self.id(),
            mapping: MappingId::from_bytes(&(mapping.identity() as u64).to_le_bytes())
                .expect("Mapping identity"),
            coordinate: index,
            generation: MemoGeneration::new(memo.map_or(1, memo::Memo::generation))
                .expect("nonzero generation"),
            phase: if source.is_some() {
                MemoPhase::Associated
            } else {
                MemoPhase::Empty
            },
            source,
            page: 0,
            access: 0,
            level: mapping.level(),
        })
    }

    /// Next Remove never starts unrelated hardware work under its atomic commit.
    pub(crate) fn remove_logical(
        &self,
        mapping: &Arc<Span>,
        index: usize,
        expected: u64,
    ) -> Result<(), NestError> {
        let _operation = mapping.operation().ok_or(NestError::Busy)?;
        self.ensure_mapping(mapping)?;
        mapping.position(index)?;
        if mapping.source().is_some() {
            return Err(NestError::Pinned);
        }
        let _commit = crate::basis::commit();
        let removed = {
            let mut state = self.state.lock();
            if !state.retirement.invalidation().is_clean() {
                return Err(NestError::Busy);
            }
            let memo = state.memos.get_mut(mapping, index)?;
            if !memo.accepts_logical() {
                return Err(NestError::Pinned);
            }
            memo.remove(expected)?
        };
        removed.release();
        Ok(())
    }

    pub(crate) fn remove_memo(
        &self,
        mapping: &Arc<Span>,
        index: usize,
        expected: u64,
    ) -> Result<(), NestError> {
        let _operation = mapping.operation().ok_or(NestError::Busy)?;
        self.ensure_mapping(mapping)?;
        mapping.position(index)?;
        if !self.is_external() {
            self.finish_invalidation()?;
        }
        self.finish_memo(mapping, index);
        let _commit = crate::basis::commit();
        let removed = {
            let mut state = self.state.lock();
            if mapping.state.lock().phase == address::span::MappingPhase::Gone {
                return Err(NestError::NoRegion);
            }
            state.memos.get_mut(mapping, index)?.remove(expected)?
        };
        removed.release();
        Ok(())
    }

    pub(crate) fn memo_published_by(
        &self,
        mapping: &Arc<Span>,
        index: usize,
        authority: &Arc<crate::basis::pend::Pend>,
    ) -> bool {
        self.state
            .lock()
            .memos
            .get(mapping, index)
            .is_some_and(|memo| memo.published_by(authority))
    }

    pub(crate) fn finish_memo(&self, mapping: &Arc<Span>, index: usize) {
        let settled = {
            let mut state = self.state.lock();
            if !state.retirement.invalidation().is_clean() {
                return;
            }
            state
                .memos
                .existing_mut(mapping, index)
                .map(memo::Memo::finish)
        };
        if let Some(settled) = settled {
            settled.release();
        }
    }

    pub(crate) fn publish_memo(
        self: &Arc<Self>,
        mapping: &Arc<Span>,
        index: usize,
        expected: u64,
        change: Arc<Change>,
        publisher: memo::Publisher,
    ) -> Result<(), kernel_api::abi::call::cell::CellFail> {
        use kernel_api::abi::call::cell::CellFail;
        let _operation = mapping.operation().ok_or(CellFail::Busy)?;
        self.ensure_mapping(mapping)?;
        if !self.is_external() {
            self.finish_invalidation()?;
        }
        self.finish_memo(mapping, index);
        let (_, access) = mapping.access(index)?;
        let principal = publisher.observe();
        let mut publisher = Some(publisher);
        let commit = crate::basis::commit();
        if !self.is_open() || !mapping.is_open() {
            return Err(CellFail::Denied);
        }
        let actor = principal.check(self, mapping, index, access)?;
        let (page, source, binding) = {
            let state = self.state.lock();
            let memo = state
                .memos
                .get(mapping, index)
                .ok_or(CellFail::SourceEmpty)?;
            if memo.generation() != expected || memo.busy() {
                return Err(CellFail::Busy);
            }
            memo.can_advance(expected)?;
            memo.association().ok_or(CellFail::SourceEmpty)?
        };
        let page = match page {
            Some(page) => page,
            None => source
                .range()?
                .page(PageOffset::ZERO)?
                .ok_or(CellFail::SourceEmpty)?,
        };
        if !Arc::ptr_eq(&page, change.page()) || page.level() != mapping.level() {
            return Err(CellFail::Busy);
        }
        source.check(access)?;
        let range = source.same_page(&page)?;
        if self.is_external() {
            if index != 0 {
                return Err(CellFail::NoRegion);
            }
            // The source loan may enumerate Nest bindings. No Nest/source lock is held.
            let mut borrow =
                Some(source.borrow(&actor, &page, access.contains(Access::WRITE), &change)?);
            if !self.is_open() || !mapping.is_open() {
                return Err(CellFail::Denied);
            }
            principal.check(self, mapping, index, access)?;
            self.prepare_external(
                mapping,
                expected,
                &page,
                change,
                access.contains(Access::WRITE),
                &mut publisher,
                &mut borrow,
            )
            .map_err(memo::publication_failure)?;
            // Responsibility is in Memo, and its loan/route are marked published
            // before the first backend write, including a partially failed submit.
            let result = self
                .submit_external(mapping)
                .map_err(memo::publication_failure);
            drop(commit);
            sign::signal(WakeKey::ExternalMapping {
                id: mapping.identity(),
            });
            return result;
        }
        let flight = source.begin_cpu(mapping.identity(), access)?;
        let lease = range.reside_during(&change)?;
        source.same_page(&page)?;
        principal.check(self, mapping, index, access)?;
        let prepared = {
            let mut state = self.state.lock();
            if !state.retirement.invalidation().is_clean() {
                return Err(CellFail::Busy);
            }
            if !state.is_open() || !mapping.is_open() {
                return Err(CellFail::Denied);
            }
            let memo = state
                .memos
                .existing_mut(mapping, index)
                .ok_or(CellFail::Denied)?;
            if !memo.matches(&binding) {
                return Err(CellFail::Busy);
            }
            memo.begin_publish(expected, change.clone())?;
            memo.retain(publisher.take().expect("prepared publishing route"), None);
            state.publish_page(
                self.geometry(),
                mapping,
                VirtAddr::new(mapping.position(index)?),
                &page,
                &change,
            )
        };
        drop(commit);
        // Neither COMMIT nor a Nest/source lock crosses a CPU fence.
        let mut result = prepared
            .and_then(|()| self.finish_invalidation())
            .map_err(CellFail::from);
        if result.is_ok() && access.contains(Access::EXECUTE) {
            result = identity::sync_instructions()
                .map_err(|error| CellFail::from(NestError::from(error)));
        }
        let valid = {
            let _commit = crate::basis::commit();
            self.is_open()
                && mapping.is_open()
                && principal.check(self, mapping, index, access).is_ok()
                && source.check(access).is_ok()
                && source.same_page(&page).is_ok()
                && source
                    .source()
                    .check_publish(source.window().0, source.window().1)
                    .is_ok()
        };
        if !valid {
            let _ = self.retract_position(mapping, index, None, change, true);
            result = Err(CellFail::Denied);
        }
        self.finish_memo(mapping, index);
        drop(lease);
        drop(flight);
        sign::signal(self.mapping_key(mapping));
        result
    }

    pub(crate) fn retract_memo(
        self: &Arc<Self>,
        mapping: &Arc<Span>,
        index: usize,
        expected: u64,
        change: Arc<Change>,
    ) -> Result<(), kernel_api::abi::call::cell::CellFail> {
        use kernel_api::abi::call::cell::CellFail;
        let _operation = mapping.operation().ok_or(CellFail::Busy)?;
        self.ensure_mapping(mapping)?;
        mapping.position(index)?;
        let result = self.retract_position(mapping, index, Some(expected), change, false);
        sign::signal(self.mapping_key(mapping));
        if self.is_external() {
            sign::signal(WakeKey::ExternalMapping {
                id: mapping.identity(),
            });
        }
        result
    }

    /// The caller owns span.operation. Forced cleanup never wraps generation MAX.
    fn retract_position(
        &self,
        mapping: &Arc<Span>,
        index: usize,
        expected: Option<u64>,
        change: Arc<Change>,
        forced: bool,
    ) -> Result<(), kernel_api::abi::call::cell::CellFail> {
        use kernel_api::abi::call::cell::CellFail;
        if !self.is_external() {
            self.finish_invalidation()?;
        }
        self.finish_memo(mapping, index);
        let page = {
            let _commit = crate::basis::commit();
            let mut state = self.state.lock();
            let Some(memo) = state.memos.existing_mut(mapping, index) else {
                return if forced {
                    Ok(())
                } else {
                    Err(CellFail::SourceEmpty)
                };
            };
            if expected.is_some_and(|expected| memo.generation() != expected) {
                return Err(CellFail::Busy);
            }
            let page = memo.page().ok_or(CellFail::SourceEmpty)?;
            if !Arc::ptr_eq(&page, change.page()) {
                return Err(CellFail::Busy);
            }
            if memo.publication().is_none() {
                return if !memo.busy() && (forced || memo.frozen()) {
                    Ok(())
                } else {
                    Err(CellFail::Busy)
                };
            }
            if !memo.retracting() {
                memo.begin_retract(memo.generation(), change, forced)?;
            }
            page
        };
        if self.is_external() {
            self.progress_external(mapping)
                .map_err(memo::publication_failure)?;
        } else {
            let result =
                self.unpublish_page(mapping, VirtAddr::new(mapping.position(index)?), &page);
            self.finish_memo(mapping, index);
            result?;
        }
        let state = self.state.lock();
        if state
            .memos
            .get(mapping, index)
            .is_some_and(memo::Memo::busy)
        {
            Err(CellFail::Busy)
        } else {
            Ok(())
        }
    }

    pub(crate) fn protect_memo(
        self: &Arc<Self>,
        mapping: &Arc<Span>,
        index: usize,
        expected: u64,
        access: Access,
        authority: &Arc<crate::basis::pend::Pend>,
    ) -> Result<(), kernel_api::abi::call::cell::CellFail> {
        use kernel_api::abi::call::cell::CellFail;
        use kernel_api::model::modifiable::Modifiable;
        use kernel_api::model::pend::{NestRights, NestScope, Operations, PendScope};
        let _operation = mapping.operation().ok_or(CellFail::Busy)?;
        self.ensure_mapping(mapping)?;
        if self.is_external() || !access.is_leaf_set() {
            return Err(CellFail::Denied);
        }
        self.finish_invalidation()?;
        self.finish_memo(mapping, index);
        let (ceiling, _) = mapping.access(index)?;
        let count = PageCount::new(mapping.grain()).ok_or(CellFail::NotAligned)?;
        let admitted = || {
            self.is_open()
                && mapping.is_open()
                && authority.permits(Operations::Nest(NestRights::PROTECT))
                && matches!(authority.scope(), PendScope::Nest(NestScope::Mapping { id, .. })
                    if id.get() == mapping.identity())
                && authority.contains(PageOffset::new(index), count)
                && (!access.contains(Access::WRITE)
                    || authority.modifiable() == Some(Modifiable::Yes))
        };
        let commit = crate::basis::commit();
        if !admitted() {
            return Err(CellFail::Denied);
        }
        let ((page, source, binding), retained) = {
            let state = self.state.lock();
            let memo = state
                .memos
                .get(mapping, index)
                .ok_or(CellFail::SourceEmpty)?;
            if memo.generation() != expected || memo.in_flight() {
                return Err(CellFail::Busy);
            }
            memo.can_advance(expected)?;
            (
                {
                    let (_, source, binding) = memo.association().ok_or(CellFail::SourceEmpty)?;
                    (memo.page().ok_or(CellFail::SourceEmpty)?, source, binding)
                },
                memo.publication().map(|publication| publication.writable()),
            )
        };
        if !ceiling.contains(access) || !source.ceiling().contains(access) {
            return Err(CellFail::WidenDenied);
        }
        let range = source.check(access)?;
        source.same_page(&page)?;
        let change = crate::basis::cell::change::acquire(&page)?;
        let flight = source.begin_cpu(mapping.identity(), access)?;
        let lease = range.reside_during(&change)?;
        let writable = access.contains(Access::WRITE);
        let admission;
        let result = {
            let mut state = self.state.lock();
            if !state.retirement.invalidation().is_clean() {
                return Err(CellFail::Busy);
            }
            if !state.is_open() || !mapping.is_open() {
                return Err(CellFail::Denied);
            }
            let memo = state
                .memos
                .existing_mut(mapping, index)
                .ok_or(CellFail::Denied)?;
            if !memo.matches(&binding) {
                return Err(CellFail::Busy);
            }
            let mut operation_freeze = if retained.is_some() {
                Some(Freeze::new(change.clone(), None).map_err(NestError::from)?)
            } else {
                None
            };
            let replacement = if retained.is_some_and(|old| old != writable) {
                Some((
                    index,
                    Publication::prepare(&page, writable, Some(&change))
                        .map_err(NestError::from)?,
                ))
            } else {
                None
            };
            // Admission and PTE edits share the checked retirement batch.
            admission = if retained.is_some() && replacement.is_none() {
                Some(change.hardware(writable).map_err(NestError::from)?)
            } else {
                None
            };
            let selection = mapping.range(PageOffset::new(index), count)?;
            state.protect(
                self.geometry(),
                mapping,
                selection.window(),
                address::span::Protection::SetAccess(access),
                replacement,
            )?;
            state
                .memos
                .existing_mut(mapping, index)
                .expect("prepared Memo")
                .protect(expected, operation_freeze.take());
            Ok::<(), CellFail>(())
        };
        drop(commit);
        let mut result = result.and_then(|()| self.finish_invalidation().map_err(CellFail::from));
        if result.is_ok() && access.contains(Access::EXECUTE) {
            result = identity::sync_instructions().map_err(CellFail::from);
        }
        drop(admission);
        let valid = {
            let _commit = crate::basis::commit();
            admitted()
                && source.check(access).is_ok()
                && source.same_page(&page).is_ok()
                && source
                    .source()
                    .check_publish(source.window().0, source.window().1)
                    .is_ok()
        };
        if !valid {
            let _ = self.retract_position(mapping, index, None, change, true);
        }
        drop(lease);
        drop(flight);
        sign::signal(self.mapping_key(mapping));
        if valid { result } else { Err(CellFail::Denied) }
    }

    fn inspect_memo(
        &self,
        mapping: &Span,
        index: usize,
    ) -> Result<kernel_api::model::cell::CellInfo, NestError> {
        let state = self.state.lock();
        if mapping.state.lock().phase == address::span::MappingPhase::Gone {
            return Err(NestError::NoRegion);
        }
        let level = if mapping.source().is_none() {
            let id = kernel_api::model::cell::MappingId::from_bytes(
                &(mapping.identity() as u64).to_le_bytes(),
            )
            .ok_or(NestError::NoRegion)?;
            self.mapping_level(id).ok_or(NestError::NoRegion)?
        } else {
            mapping.level()
        };
        let address = mapping.position(index)?;
        let mut info = state
            .memos
            .get(mapping, index)
            .unwrap_or(&memo::Memo::vacant())
            .inspect(VirtAddr::new(address));
        info.level = level;
        Ok(info)
    }

    /// 撤下 `mapping` 在 `address` 上的 4KiB 映射，把它那一页的承载交给退休。
    ///
    /// - 内核空间的物理使用不由撤下本关系结清；全局条目不受 ASID 定向失效覆盖。
    ///   两类都不参与移交，当场 `Pinned`。
    /// - 先结清本 Nest 之前那次失效：旧 TLB 没确认就不再撤新的。
    /// - 在 `Nest → Span` 同一个临界区里认这一页：只有真有一条发布记录才算这条
    ///   关系握着它；没有记录（活动洞或本来就没发布）就没有可撤的映射，当场
    ///   成功返回。真握着的话，它必须正是 `expected`——不是就在动任何 PTE 之前
    ///   当场断言，不先撤再发现。
    /// - 容量与大叶拆分都在撤下之前备齐，之后不再有可失败步骤。
    /// - 承载只在 shootdown 成功之后随退休一起放掉；失败时这一页的退休责任
    ///   留在本 Nest，由 `retry_invalidation` 继续。
    pub(crate) fn unpublish_page(
        &self,
        mapping: &Arc<Span>,
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
                .is_some_and(|owner| core::ptr::eq(Arc::as_ptr(&owner), self)),
            "unpublish_page requires the mapping's own live nest"
        );
        self.finish_invalidation()?;

        let mut state = self.state.lock();
        if !state.retirement.invalidation().is_clean() {
            return Err(NestError::Busy);
        }
        if !state.unpublish_page(self.geometry(), mapping, address, expected)? {
            return Ok(());
        }
        drop(state);
        // 退休的承载只在这次确认之后才随 `finish_invalidation` 放掉。
        self.finish_invalidation()?;
        self.finish_memo(mapping, mapping.page_index(address));
        Ok(())
    }

    /// 让 `Flat` 物化该窗口的活动页，再在本关系发布。
    ///
    /// 组合入口：物化原语仍是 `FlatRange::materialize`，这里只把缺页与派生编排
    /// 要的两步（先 Flat、再本关系 publish）接在一起，不是第二套物化权威。
    pub(crate) fn materialize_active(
        self: &Arc<Self>,
        selection: NestRange,
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
    fn active_spans(&self, selection: &NestRange) -> Result<Vec<(VirtAddr, usize)>, NestError> {
        let mapping = selection.mapping();
        let (lo, hi) = selection.window();
        let state = self.ready_state()?;
        self.ensure_owned(mapping, &state)?;
        let inner = mapping.state.lock();
        if inner.active.is_empty()
            || (matches!(selection, NestRange::Pages { .. }) && !inner.covers(lo, hi))
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

    /// 收窄选择窗口的权限上限与当前权限；架构属性不变。
    pub(crate) fn narrow(&self, selection: NestRange, ceiling: Access) -> Result<(), NestError> {
        let key = self.mapping_key(selection.mapping());
        let result = self.protect(selection, address::span::Protection::Narrow(ceiling));
        sign::signal(key);
        result
    }

    /// 纯决策在同一 Nest / Span 锁内消费，不允许准备与提交间出现过期状态。
    fn protect(
        &self,
        selection: NestRange,
        request: address::span::Protection,
    ) -> Result<(), NestError> {
        let mapping = selection.mapping().clone();
        let mut state = self.ready_state()?;
        self.ensure_owned(&mapping, &state)?;
        state.protect(self.geometry(), &mapping, selection.window(), request, None)?;
        drop(state);
        self.finish_invalidation()
    }

    fn mapping_key(&self, mapping: &Span) -> WakeKey {
        WakeKey::Mapping {
            space: self.id,
            mapping: mapping.identity(),
        }
    }

    pub(crate) fn detach(self: &Arc<Self>, selection: NestRange) -> Result<(), NestError> {
        let key = self.mapping_key(selection.mapping());
        let result = self.detach_range(selection);
        sign::signal(key);
        result
    }

    /// 按选择撤下：先退休该段占用，再撤 PTE，同步成功后只回收这一段。
    ///
    /// 失败时退休票、Page、表页、来源与待撤的活动全都保留，VA 禁复用；
    /// 沿用同一身份重试（再次 `detach`，或 `retry_invalidation`）才继续。
    fn detach_range(self: &Arc<Self>, selection: NestRange) -> Result<(), NestError> {
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
    fn owed_spans(&self, selection: &NestRange) -> Result<Vec<(VirtAddr, usize)>, NestError> {
        let mapping = selection.mapping();
        let (lo, hi) = selection.window();
        let strict = matches!(selection, NestRange::Pages { .. });
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
        mapping: &Arc<Span>,
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

        // 地址 → Nest → Span：准备与提交共享临界区，不暴露无人接管的退休票。
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
        let removed = state
            .memos
            .published(mapping)
            .filter(|(p, _)| *p >= first_page && *p <= last_page)
            .count();
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
        state.prepare_withdrawal(start, len, self.geometry())?;

        // 开票之后没有可失败准备：票、PTE 与关系在同一提交内转入退休责任。
        for (lo, hi, ticket) in pieces {
            tickets.push(if ticket == 0 {
                layout.occupancy.retire(lo, hi, mapping.reservation.id())?
            } else {
                ticket
            });
        }
        state.withdraw_prepared(start, len, self.geometry());
        state.retirement.mappings.push(mapping.clone());
        {
            let NestState {
                memos, retirement, ..
            } = &mut *state;
            memos.withdraw(mapping, first_page, last_page, &mut retirement.cells);
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
    pub(crate) fn dispose(self: &Arc<Self>, mapping: Arc<Span>) -> Result<(), NestError> {
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
    /// 关系收到手。正常关闭别的 Span 不会走到这里，因此也不会错误结清失败的那段。
    pub(crate) fn retry_invalidation(self: &Arc<Self>) -> Result<(), NestError> {
        self.finish_invalidation()?;
        if self.is_closed() {
            self.drain_closed();
            return Ok(());
        }
        if self.state.lock().retirement.tickets.has_settled() {
            let mut layout = self.layout.lock();
            let mut state = self.state.lock();
            for ticket in state.retirement.tickets.drain(4) {
                layout.occupancy.reclaim(ticket);
            }
        }
        for _ in 0..4 {
            let Some(node) = self.take_cleanup() else {
                break;
            };
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
        if self.pending_cleanup.lock().is_some()
            || self.state.lock().retirement.tickets.has_settled()
        {
            crate::basis::room::deadline::retry();
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
                // Keep synchronous callers' access result stable while a peer
                // fences. Reception uses the stack-free interrupt window.
                crate::metal::interrupt::window::poll();
                continue;
            }
            let target = state.retirement.begin(self.id.get(), self.asid())?;
            drop(state);

            let result = fence::invalidate(
                &self.cpu.as_ref().ok_or(NestError::NoRegion)?.domain,
                target,
            );
            let fenced = {
                let mut state = self.state.lock();
                match result {
                    Ok(proof) => state.settle(proof)?,
                    Err(_) => {
                        state.retirement.fail();
                        return Err(NestError::Shootdown);
                    }
                }
            };
            // Retired Page and table resources can leave the lock. Address
            // tickets are reclaimed by detach or explicit retry_invalidation.
            fenced.release(self);
            return Ok(());
        }
    }

    /// 撤下全部关系并等算 shootdown；内核地址空间为进程期，不在此列。
    fn close(&self) -> Result<(), NestError> {
        if self.cpu.is_none() {
            return Err(NestError::Busy);
        }
        if self.asid().is_kernel() {
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
        state.close_mappings(self.geometry())?;
        drop(state);
        self.finish_invalidation()?;
        self.drain_closed();
        if self.is_closed() {
            Ok(())
        } else {
            Err(NestError::Busy)
        }
    }

    /// Close translations, clear waits for this ASID, and settle control bindings
    /// before the identifier can be reused. A failed release keeps it for retry.
    pub(crate) fn retire(&self) -> Result<(), NestError> {
        if !self.cpu.as_ref().is_none_or(|cpu| cpu.domain.released()) {
            self.close()?;
            self.drain_mapping_resources()?;
            if self.pending_cleanup.lock().is_some() {
                return Err(NestError::Busy);
            }
            if !sign::wipe_space(self.id) {
                return Err(NestError::Busy);
            }
            if !self.reap_mappings() {
                return Err(NestError::Busy);
            }
            self.release_asid()?;
        }
        self.reap_mappings();
        Ok(())
    }

    fn release_asid(&self) -> Result<(), NestError> {
        if !self.is_closed() {
            return Err(NestError::Busy);
        }
        self.cpu
            .as_ref()
            .ok_or(NestError::NoRegion)?
            .domain
            .release()
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
        if !self.is_closed() {
            return;
        }
        for _ in 0..4 {
            let Some(node) = self.take_cleanup() else {
                break;
            };
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
    fn ensure_owned(&self, mapping: &Arc<Span>, state: &NestState) -> Result<(), NestError> {
        let owner = mapping.reservation.nest().ok_or(NestError::NoRegion)?;
        if state.phase.is_closed()
            || !core::ptr::eq(Arc::as_ptr(&owner) as *const Nest, self as *const Nest)
        {
            return Err(NestError::NoRegion);
        }
        Ok(())
    }

    fn ensure_mapping(&self, mapping: &Arc<Span>) -> Result<(), NestError> {
        self.check_owner(&mapping.reservation)?;
        if self
            .mappings
            .registered(mapping.identity())
            .and_then(|entry| entry.mapping)
            .is_none_or(|held| !Arc::ptr_eq(&held, mapping))
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

    pub(crate) fn is_closed(&self) -> bool {
        self.state.lock().phase.is_closed()
    }

    /// 翻译里只取物理地址的那一面：权限位不外露。
    pub(crate) fn physical_at(&self, address: VirtAddr) -> Option<PhysAddr> {
        self.translate(address).map(|(physical, _)| physical)
    }

    pub(crate) fn translate(&self, address: VirtAddr) -> Option<(PhysAddr, Attributes)> {
        let state = self.ready_state().ok()?;
        state.index.find(address.as_usize())?;
        let (pa, flags) = state
            .root
            .cpu()
            .ok()?
            .walk_ref_geometry(address, self.geometry())
            .ok()?;
        Some((pa + address.offset(), flags))
    }

    /// 用户页错误的纯一致探针：只读当前的身份、活动权限、承载与硬件条目。
    ///
    /// 直取 `Nest → Span → Flat` 三层状态，不推进失效、不物化、不分配、不新造
    /// 页状态，因此可以在 work 的提交边界内作为条件等待的谓词跑一次。
    /// 待结算的失效先归到 `Materialize`：那份失效一旦真的未确认，旧 PTE 就不是
    /// “已经可用”的证据，也不能据此把一个已经退场的关系判死。
    /// 承载先于 PTE 看：`Fresh`/`Resident`/有效借用再看条目，满足才是 `Resolved`；
    /// `Missing`/`Moving`/`Reserved` 一律 `Wait`，不因旧 PTE 还在就绕过等待；
    /// 身份消失或活动权限不容纳这次访问是 `Invalid`。
    pub(crate) fn probe_fault(&self, address: VirtAddr, kind: FaultKind) -> FaultOutcome {
        FaultOutcome::probe(&self.state.lock(), self.geometry(), address, kind)
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
                PageOffset::new(mapping.page_index(page) / mapping.grain() * mapping.grain()),
                PageCount::new(mapping.grain()).expect("one fault Cell"),
            )?
        };
        self.materialize_active(selection)
    }

    #[cfg(debug_assertions)]
    pub(crate) fn audit(&self) {
        self.state.lock().audit(self, self.geometry());
    }
}

impl Drop for Nest {
    fn drop(&mut self) {
        if self.cpu.is_none() {
            if self.external_settled() {
                unsafe { ManuallyDrop::drop(&mut self.state) };
            }
            return;
        }
        if !self.cpu.as_ref().expect("CPU drop").domain.published() {
            // Backend Drop returns an unpublished ASID.
        } else if self.asid().is_kernel() {
            return;
        } else if !self.cpu.as_ref().is_none_or(|cpu| cpu.domain.released()) {
            if !sign::wipe_space(self.id) {
                return;
            }
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
