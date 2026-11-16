//! 地址翻译：地址占用、翻译关系、硬件后端、身份与同步退休。
//!
//! Flat 继续独立管理来源与物化；这里不保存来源页槽。

mod access;
pub(crate) mod activity;
pub(crate) mod address;
mod admission;
pub(crate) mod authority;
mod error;
pub(crate) mod index;
pub(crate) mod memo;
pub(crate) mod retirement;
mod translation;
pub(crate) mod wait;

use alloc::boxed::Box;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::mem::ManuallyDrop;

use crate::basis::cell::page::Page;
use crate::basis::flat::Pages;
use crate::basis::sign::{self, WakeKey};
use crate::basis::sync::LegacyGuard;
use crate::basis::sync::LegacyLock;
use crate::basis::unit::anchor::Anchor;
use crate::metal::generic::mmu::internal::{Attributes, Domain, PageTable, fence};
use crate::metal::mmu::internal::{self as mmu, Geometry, Root, Table, identity};
use kernel_api::address::PAGE_SIZE;
use kernel_api::address::PageCount;
use kernel_api::address::PageOffset;
use kernel_api::address::PhysAddr;
use kernel_api::address::VirtAddr;
use translation::{Invalidation, Translation};

use crate::metal::generic::trap::FaultKind;
pub(crate) use address::PreparedSpan;
pub use address::span::{Access, ActiveRange, Span};
use address::span::{SpanIdentity, SpanRange};
pub use error::NestError;
pub use identity::Asid;
pub use translation::FaultOutcome;
pub(crate) mod fault;

pub(crate) use address::NestRange;
use address::{AddressState, Layout};
pub(crate) use address::{Placement, PlacementAt, RegionKind};
use admission::Admission;

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
/// Nest owns admission and address organization and holds the shared
/// Translation; each Span owns its Memos and holds that same Translation.
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
    revision: core::sync::atomic::AtomicU64,
    cpu: Option<CpuContext>,
    life: Arc<Anchor>,
    admission: Arc<Admission>,
    pub(crate) spans: authority::SpanIndex,
    address: LegacyLock<crate::basis::sync::order::Address, AddressState>,
    pending_cleanup: LegacyLock<crate::basis::sync::order::Cleanup, Option<Box<CleanupNode>>>,
    state: ManuallyDrop<Arc<Translation>>,
}

impl Nest {
    pub(crate) fn maintenance_failed(
        &self,
        span: &Arc<Span>,
        index: usize,
        usage: &Arc<crate::basis::pend::MaintenanceUse>,
    ) -> bool {
        let state = span.translation.lock();
        let inner = span.state.lock();
        inner.memos.get(index).is_some_and(|memo| {
            memo.maintenance()
                .is_some_and(|known| Arc::ptr_eq(known, usage))
                && memo.failed(state.retirement.invalidation() == Invalidation::Failed)
        })
    }
    pub(crate) fn retire_maintenance(
        &self,
        span: &Arc<Span>,
        index: usize,
        generation: u64,
        usage: &Arc<crate::basis::pend::MaintenanceUse>,
    ) -> Result<(), kernel_api::abi::call::pend::PendFail> {
        use kernel_api::abi::call::pend::PendFail;
        let _operation = span.operation().ok_or(PendFail::Busy)?;
        {
            let _commit = crate::basis::commit();
            let state = span.state.lock();
            if state.memos.get(index).is_none_or(|memo| {
                memo.generation() < generation
                    || memo
                        .maintenance()
                        .is_none_or(|known| !Arc::ptr_eq(known, usage))
            }) {
                return Ok(());
            }
        }
        span.retract_forced(self, index, usage.permit().change().clone())
            .map_err(|_| PendFail::Busy)
    }
    /// Called under COMMIT after live whole-resource authority/version checks.
    pub(crate) fn begin_delete(&self) -> Result<(), NestError> {
        if !self.is_external() && (self.is_adopted() || self.asid().is_kernel()) {
            return Err(NestError::Pinned);
        }
        self.admission.close();
        let address = self.address.lock();
        let mut state = self.state.lock();
        if state.phase.is_open() {
            self.version().next().ok_or(NestError::OutOfMemory)?;
            if let translation::Storage::External(external) = &mut state.root {
                external.close();
            }
            state.phase = translation::phase::ClosePhase::Draining;
            address.index.close_faults();
            self.changed();
        }
        drop(state);
        fault::wake_nest(self);
        Ok(())
    }

    /// Closing admission is separate from draining hardware or reusing addresses.
    pub(crate) fn begin_release(
        &self,
        span: &Arc<Span>,
        expected: kernel_api::model::nest::SpanVersion,
    ) -> Result<(), NestError> {
        self.check_owner(span)?;
        let _state = self.state.lock();
        if span.version() != expected {
            return Err(NestError::Busy);
        }
        if span.is_open() {
            span.prepare_revision()?;
            self.version().next().ok_or(NestError::OutOfMemory)?;
            span.close();
            self.changed();
        }
        drop(_state);
        fault::wake_span(span);
        Ok(())
    }
    pub(crate) fn version(&self) -> kernel_api::model::nest::NestVersion {
        kernel_api::model::nest::NestVersion::new(
            self.revision.load(core::sync::atomic::Ordering::Acquire),
        )
        .expect("Nest revision")
    }
    pub(crate) fn changed(&self) {
        let _ = self.revision.try_update(
            core::sync::atomic::Ordering::AcqRel,
            core::sync::atomic::Ordering::Acquire,
            |v| v.checked_add(1),
        );
    }

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
        let plan = crate::metal::layout::plan();
        layout.register_window(RegionKind::Normal, plan.normal.base, plan.normal.last)?;
        layout.register_window(RegionKind::Kernel, plan.team.base, plan.team.last)?;
        if kernel {
            layout.register_window(RegionKind::Resource, plan.resource.base, plan.resource.last)?;
        }
        let life = Anchor::try_new()?;
        let id = next_id()?;
        let domain = mmu::Domain::create(id.get(), kernel)?;
        let nest = Arc::try_new(Self {
            id,
            revision: core::sync::atomic::AtomicU64::new(1),
            cpu: Some(CpuContext { domain, kind }),
            life,
            admission: Admission::new()?,
            spans: authority::SpanIndex::new(),
            address: LegacyLock::new(AddressState::new(layout)),
            pending_cleanup: LegacyLock::new(None),
            state: ManuallyDrop::new(Translation::new(NestState::new(root))?),
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
    fn ready_state(&self) -> Result<LegacyGuard<'_, NestState>, NestError> {
        // One covering fence per turn. A peer may dirty a new batch before the
        // final lock acquisition; admission then returns Busy for continuation.
        for turn in 0..2 {
            let state = self.state.lock();
            if !self.admission.is_open() || !state.phase.is_open() {
                return Err(NestError::NoRegion);
            }
            match state.retirement.invalidation() {
                Invalidation::Clean => return Ok(state),
                Invalidation::Failed => return Err(NestError::Shootdown),
                Invalidation::Dirty | Invalidation::Fencing => {}
            }
            if turn == 1 {
                return Err(NestError::Busy);
            }
            drop(state);
            self.finish_invalidation()?;
        }
        unreachable!("bounded admission")
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
        self.admission.is_open()
    }

    pub(crate) fn is_adopted(&self) -> bool {
        self.address.lock().team.is_some()
    }

    /// The Team becomes the CPU Nest's lifetime owner exactly once.
    pub(crate) fn attach_team(
        &self,
        team: kernel_api::model::identity::TeamId,
    ) -> Result<(), NestError> {
        if self.is_external() {
            return Err(NestError::NoRegion);
        }
        let mut address = self.address.lock();
        if address.team.is_some() {
            return Err(NestError::Busy);
        }
        let state = self.state.lock();
        if !state.is_open() || !state.retirement.invalidation().is_clean() || !self.spans.empty() {
            return Err(NestError::Busy);
        }
        address.team = Some(team);
        Ok(())
    }

    pub(crate) fn span(&self, id: kernel_api::model::nest::SpanId) -> Option<Arc<Span>> {
        self.spans.registered(id.get())?.span
    }

    pub(crate) fn span_open(&self, id: kernel_api::model::nest::SpanId) -> bool {
        self.is_open() && self.span(id).is_some_and(|mapping| mapping.is_open())
    }

    pub(crate) fn span_status(
        &self,
        id: kernel_api::model::nest::SpanId,
    ) -> kernel_api::model::pend::PendStatus {
        use address::span::SpanPhase;
        use kernel_api::model::pend::PendStatus;
        match self.span(id).map(|mapping| mapping.state.lock().phase) {
            Some(SpanPhase::Open) if self.is_open() => PendStatus::Active,
            Some(SpanPhase::Open | SpanPhase::Closing) => PendStatus::Retiring,
            Some(SpanPhase::Gone) | None => PendStatus::Dead,
        }
    }

    /// Sample phase and revision under the same owner lock as physical completion.
    /// The caller holds COMMIT while selecting the live authority window.
    pub(crate) fn span_snapshot(
        &self,
        span: &Arc<Span>,
        offset: PageOffset,
        count: PageCount,
        writable: bool,
    ) -> Result<kernel_api::model::nest::SpanStatus, NestError> {
        use kernel_api::model::nest::{SpanId, SpanPhase, SpanStatus};
        self.check_owner(span)?;
        let state = self.state.lock();
        let inner = span.state.lock();
        let phase = match inner.phase {
            address::span::SpanPhase::Open if state.is_open() => SpanPhase::Open,
            address::span::SpanPhase::Gone => SpanPhase::Closed,
            _ => SpanPhase::Closing,
        };
        let mut access = inner.initial_access;
        if !writable {
            access.remove(Access::WRITE);
        }
        Ok(SpanStatus {
            nest: self.id(),
            span: SpanId::from_bytes(&(span.identity() as u64).to_le_bytes())
                .expect("span identity"),
            version: span.version(),
            offset: offset.get(),
            count: count.get(),
            address: span.initial_start(),
            level: span.level(),
            access: access.bits() << 1,
            phase,
        })
    }

    pub(crate) fn span_root(
        &self,
        id: kernel_api::model::nest::SpanId,
        root: &Arc<crate::basis::pend::Pend>,
    ) -> bool {
        self.spans.registered(id.get()).is_some_and(|entry| {
            entry
                .root
                .as_ref()
                .is_some_and(|known| known.authority.ptr_eq(&Arc::downgrade(root)))
        })
    }

    pub(crate) fn span_count(&self, id: kernel_api::model::nest::SpanId) -> Option<PageCount> {
        PageCount::new(self.span(id)?.initial_size() / PAGE_SIZE)
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
        let address = self.address.lock();
        Ok(address.index.first_overlap(start.as_usize(), end).is_some())
    }

    pub(crate) fn has_pending_invalidation(&self) -> bool {
        !self.state.lock().retirement.invalidation().is_clean()
    }

    pub(crate) fn contains_span(&self, id: usize) -> bool {
        self.address.lock().occupancy.has_id(id)
    }

    /// 原身份的占用票已撤下，或全空间关闭的失效同步已完成。
    pub(crate) fn settled(&self, id: usize) -> bool {
        self.is_closed() || !self.contains_span(id)
    }

    /// Retire a moved Page through its retained original source, without
    /// clearing an unrelated logical association or trusting a stale binding.
    pub(crate) fn withdraw_provenance(
        &self,
        binding: &Arc<authority::RangeBinding>,
        origin: &Arc<crate::basis::pend::Provenance>,
        block: Option<&crate::basis::pend::RangeBlock>,
    ) -> Result<(), kernel_api::abi::call::pend::PendFail> {
        use kernel_api::abi::call::pend::PendFail;
        let Some(index) = binding.page() else {
            return Ok(());
        };
        let Some(record) = self.spans.registered(binding.id()) else {
            return Ok(());
        };
        let Some(span) = record.span() else {
            return Err(PendFail::Busy);
        };
        let _operation = span.operation().ok_or(PendFail::Busy)?;
        let change = {
            let _commit = crate::basis::commit();
            let state = span.state.lock();
            let Some(memo) = state.memos.get(index) else {
                return Ok(());
            };
            if !memo.matches(binding) {
                return Ok(());
            }
            if memo.provenance().is_none_or(|p| !p.contains(origin)) {
                return Ok(());
            }
            let Some(page) = memo.page() else {
                return Ok(());
            };
            if let Some(block) = block {
                block.retain(page.clone())?;
            }
            crate::basis::cell::change::acquire(&page).map_err(|_| PendFail::Busy)?
        };
        span.retract_forced(self, index, change)
            .map_err(|_| PendFail::Busy)
    }
    pub(crate) fn binding_source(
        &self,
        binding: &Arc<authority::RangeBinding>,
    ) -> Option<memo::source::Source> {
        let index = binding.page()?;
        let mapping = self.spans.registered(binding.id())?.span?;
        let state = mapping.state.lock();
        let memo = state.memos.get(index)?;
        if !memo.matches(binding) {
            return None;
        }
        memo.association().map(|(_, source, _)| source)
    }

    pub(crate) fn binding_failed(&self, binding: &Arc<authority::RangeBinding>) -> bool {
        let Some(index) = binding.page() else {
            return false;
        };
        let Some(record) = self.spans.registered(binding.id()) else {
            return false;
        };
        let Some(span) = record.span() else {
            return false;
        };
        let state = self.state.lock();
        let inner = span.state.lock();
        inner.memos.get(index).is_some_and(|memo| {
            memo.matches(binding)
                && memo.failed(state.retirement.invalidation() == Invalidation::Failed)
        })
    }

    pub(crate) fn provenance_failed(
        &self,
        binding: &Arc<authority::RangeBinding>,
        origin: &crate::basis::pend::Provenance,
    ) -> bool {
        let Some(index) = binding.page() else {
            return false;
        };
        let Some(record) = self.spans.registered(binding.id()) else {
            return false;
        };
        let Some(span) = record.span() else {
            return false;
        };
        let state = self.state.lock();
        let inner = span.state.lock();
        inner.memos.get(index).is_some_and(|memo| {
            memo.matches(binding)
                && memo.provenance().is_some_and(|p| p.contains(origin))
                && memo.failed(state.retirement.invalidation() == Invalidation::Failed)
        })
    }
    pub(crate) fn binding_pending(&self, binding: &Arc<authority::RangeBinding>) -> bool {
        let Some(entry) = self.spans.registered(binding.id()) else {
            return false;
        };
        let Some(mapping) = entry.span() else {
            return binding.page().is_none() && Arc::ptr_eq(&entry.binding, binding);
        };
        let state = self.state.lock();
        let inner = mapping.state.lock();
        if let Some(index) = binding.page() {
            return inner
                .memos
                .get(index)
                .is_some_and(|memo| memo.matches(binding) && memo.busy());
        }
        let borrowed = !inner.active.is_empty()
            && inner
                .source
                .as_ref()
                .is_some_and(|source| source.is_borrowed());
        Arc::ptr_eq(&entry.binding, binding)
            && !state.phase.is_closed()
            && (!state.retirement.invalidation().is_clean()
                || inner.memos.published().next().is_some()
                || borrowed)
    }

    /// Close reservation admission before contending for its operation gate.
    pub(crate) fn close_span(
        &self,
        id: kernel_api::model::nest::SpanId,
    ) -> Result<(), crate::basis::nest::memo::AccessError> {
        use crate::basis::nest::memo::AccessError;
        use address::span::{DrainStep, SpanPhase};
        let Some(mapping) = self.span(id) else {
            return Ok(());
        };
        {
            let _commit = crate::basis::commit();
            if mapping.is_open() {
                self.changed();
            }
            mapping.close();
        }
        fault::wake_span(&mapping);
        let _operation = mapping.operation().ok_or(AccessError::Busy)?;
        if mapping.state.lock().phase == SpanPhase::Gone {
            return Ok(());
        }
        if mapping.source().is_some() {
            return Err(AccessError::Denied);
        }
        if !self.is_external() && self.state.lock().retirement.pending_for(&mapping) {
            self.finish_invalidation()?;
        }
        loop {
            match mapping.drain_one(self)? {
                DrainStep::Done => break,
                DrainStep::Progress => {}
                DrainStep::Blocked => return Err(AccessError::Busy),
            }
        }
        // Own Memos have been removed, including same-Span exports.
        // Foreign destinations must retire before this address can be reused.
        mapping.withdraw_exports()?;
        self.reclaim(&mapping)?;
        sign::signal(self.span_key(&mapping));
        if self.is_external() {
            sign::signal(WakeKey::ExternalSpan { id: id.get() });
        }
        Ok(())
    }

    /// Return a fully drained Span's address to the Nest authority.
    fn reclaim(&self, mapping: &Arc<Span>) -> Result<(), crate::basis::nest::memo::AccessError> {
        use crate::basis::nest::memo::AccessError;
        use address::span::SpanPhase;
        let _commit = crate::basis::commit();
        let mut address = self.address.lock();
        let state = self.state.lock();
        // An unrelated batch may already own a newer fence. Only this
        // mapping's retained batch and resources prevent its release.
        if state.retirement.pending_for(mapping) || mapping.state.lock().memos.has_resources() {
            return Err(AccessError::Busy);
        }
        address.index.remove(mapping);
        address.occupancy.release_settled(mapping.identity());
        let mut inner = mapping.state.lock();
        inner.active.clear();
        inner.phase = SpanPhase::Gone;
        mapping.changed();
        Ok(())
    }

    pub(crate) fn withdraw_span(
        self: &Arc<Self>,
        id: kernel_api::model::nest::SpanId,
        window: (PageOffset, PageCount),
        authority: &Arc<crate::basis::pend::Pend>,
    ) -> Result<(), kernel_api::abi::call::pend::PendFail> {
        use kernel_api::abi::call::pend::PendFail;
        use kernel_api::model::pend::{NestScope, PendScope};
        let Some(mapping) = self.span(id) else {
            return Ok(());
        };
        let _operation = mapping.operation().ok_or(PendFail::Busy)?;
        mapping
            .range(window.0, window.1)
            .map_err(|_| PendFail::NotAligned)?;
        if !matches!(authority.scope(), PendScope::Nest(NestScope::Span { id: known, .. }) if known == id)
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
            let Some(index) = mapping.state.lock().memos.next_page(cursor) else {
                break;
            };
            cursor = Some(index);
            if index < window.0.get() || index >= end {
                continue;
            }
            mapping.finish_memo(index);
            if !mapping.memo_published_by(index, authority) {
                continue;
            }
            let Some(page) = mapping.memo_page(index).map_err(|_| PendFail::Busy)? else {
                busy = true;
                continue;
            };
            match crate::basis::cell::change::acquire(&page) {
                Ok(change) => busy |= mapping.retract_forced(self, index, change).is_err(),
                Err(_) => busy = true,
            }
        }
        sign::signal(self.span_key(&mapping));
        if self.is_external() {
            sign::signal(WakeKey::ExternalSpan { id: id.get() });
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
        self.withdraw_source_with(binding, authorization, token, actor, clear, false)
    }

    pub(crate) fn poll_withdraw_source(
        self: &Arc<Self>,
        binding: &Arc<authority::RangeBinding>,
        authorization: &Arc<crate::basis::pend::SourceAuthorization>,
    ) -> Result<(), kernel_api::abi::call::pend::PendFail> {
        if self.is_external() {
            return Err(kernel_api::abi::call::pend::PendFail::Denied);
        }
        self.withdraw_source_with(binding, authorization, None, None, true, true)
    }

    fn withdraw_source_with(
        self: &Arc<Self>,
        binding: &Arc<authority::RangeBinding>,
        authorization: &Arc<crate::basis::pend::SourceAuthorization>,
        token: Option<kernel_api::model::identity::PendId>,
        actor: Option<kernel_api::model::identity::TaskId>,
        clear: bool,
        polling: bool,
    ) -> Result<(), kernel_api::abi::call::pend::PendFail> {
        use kernel_api::abi::call::pend::PendFail;
        let Some(entry) = self.spans.registered(binding.id()) else {
            return Ok(());
        };
        let Some(mapping) = entry.span() else {
            return Err(PendFail::Busy);
        };
        let _operation = mapping.operation().ok_or(PendFail::Busy)?;
        let Some(index) = binding.page() else {
            if polling {
                return Err(PendFail::Busy);
            }
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
                self.retire_span(binding.id());
                return Ok(());
            }
            if mapping.source().is_some_and(|source| source.is_borrowed()) {
                return Err(PendFail::Busy);
            }
            let mut cursor = None;
            loop {
                let Some(index) = mapping.state.lock().memos.next_page(cursor) else {
                    break;
                };
                cursor = Some(index);
                let Some(page) = mapping.memo_page(index).map_err(|_| PendFail::Busy)? else {
                    continue;
                };
                self.unpublish_page(
                    mapping,
                    VirtAddr::new(mapping.position(index).map_err(|_| PendFail::Busy)?),
                    &page,
                )
                .map_err(|_| PendFail::Busy)?;
                mapping.finish_memo(index);
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
            let state = mapping.state.lock();
            let Some(memo) = state.memos.get(index) else {
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
            self.finish_invalidation_with(polling)
                .map_err(|_| PendFail::Busy)?;
        }
        mapping.finish_memo(index);
        let pending = mapping
            .state
            .lock()
            .memos
            .get(index)
            .is_some_and(memo::Memo::busy);
        if pending {
            let page = page.ok_or(PendFail::Busy)?;
            let change = crate::basis::cell::change::acquire(&page).map_err(|_| PendFail::Busy)?;
            mapping
                .retract_polling(self, index, change, polling)
                .map_err(|_| PendFail::Busy)?;
        }
        if clear {
            let cleared = {
                let _commit = crate::basis::commit();
                // Revalidate the exact shared binding; a stale (id,page) cannot clear a replacement.
                let mut state = mapping.state.lock();
                let Some(memo) = state.memos.existing_mut(index) else {
                    return Ok(());
                };
                if !memo.matches(binding) {
                    return Ok(());
                }
                let removed = memo.clear().map_err(|_| PendFail::Busy)?;
                mapping.changed();
                removed
            };
            cleared.release();
        }
        sign::signal(self.span_key(mapping));
        if self.is_external() {
            sign::signal(WakeKey::ExternalSpan {
                id: mapping.identity(),
            });
        }
        Ok(())
    }

    /// Prepare a source-free Span; only publication makes it visible.
    pub(crate) fn prepare_span(
        self: &Arc<Self>,
        placement: Placement,
        level: kernel_api::model::cell::CellLevel,
        access: Access,
        creator: kernel_api::model::identity::TaskId,
    ) -> Result<PreparedSpan, NestError> {
        self.reap_spans();
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
            if self.spans.competing_span(creator) {
                return Err(NestError::Busy);
            }
        } else if level.get() >= crate::metal::mmu::internal::mode::levels() {
            return Err(NestError::NotAligned);
        }
        if !self.is_open() {
            return Err(NestError::NoRegion);
        }
        let span = self.reserve(placement, level)?;
        let active =
            match ActiveRange::new(span.initial_start(), span.initial_size(), access, access) {
                Ok(active) => active,
                Err(error) => {
                    self.give_back(&span);
                    return Err(error);
                }
            };
        let mut ranges = Vec::new();
        if ranges.try_reserve(1).is_err() {
            self.give_back(&span);
            return Err(NestError::OutOfMemory);
        }
        ranges.push(active);
        if let Err(error) = span.stage(ranges) {
            self.give_back(&span);
            return Err(error);
        }
        PreparedSpan::new(self, span, creator)
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
        Ok(self.address.lock().advance_window(region, base)?)
    }

    /// 该区域的动态窗口是否含住 `[start, start + size)`。
    pub(crate) fn window_contains(&self, region: RegionKind, start: VirtAddr, size: usize) -> bool {
        let Some(last) = start.as_usize().checked_add(size - 1) else {
            return false;
        };
        self.address
            .lock()
            .window_contains(region, start.as_usize(), last)
    }

    /// 选一段跨度并登记占用；不动页表、不物化来源。
    pub(crate) fn reserve(
        self: &Arc<Self>,
        placement: Placement,
        level: kernel_api::model::cell::CellLevel,
    ) -> Result<Arc<Span>, NestError> {
        let attrs = self.attrs(placement.region());
        self.reserve_with(placement, level, attrs)
    }

    /// 选一段跨度并登记占用，使用调用方给出的架构属性。
    pub(crate) fn reserve_with(
        self: &Arc<Self>,
        placement: Placement,
        level: kernel_api::model::cell::CellLevel,
        attrs: Attributes,
    ) -> Result<Arc<Span>, NestError> {
        let (raw, id) = self.address.lock().reserve(placement)?;
        let start = if self.cpu.is_some() {
            crate::metal::mmu::internal::canonical(raw)
        } else {
            VirtAddr::new(raw)
        };
        let range = match SpanRange::new(start, placement.size(), level) {
            Ok(range) => range,
            Err(error) => {
                self.give_back_span(VirtAddr::new(raw), placement.size(), id);
                return Err(error);
            }
        };
        Span::vacant(self, SpanIdentity::new(id, self), range, attrs)
    }

    /// 普通挂载：整段初始范围一条活动记录，属性按种类与区域政策算出。
    pub(crate) fn attach(
        self: &Arc<Self>,
        span: Arc<Span>,
        source: crate::basis::flat::FlatRange,
        ceiling: Access,
        current: Access,
    ) -> Result<Arc<Span>, NestError> {
        if let Err(error) = self.check_owner(&span) {
            return Err(self.abandon(span, error));
        }
        let active =
            match ActiveRange::new(span.initial_start(), span.initial_size(), ceiling, current) {
                Ok(active) => active,
                Err(error) => return Err(self.abandon(span, error)),
            };
        self.attach_shaped(span, source, &[active])
    }

    /// 按给定活动形状挂载：与普通入口共用一个私有安装/提交。
    ///
    /// 只接真实占用、真实来源与明确的活动区间；不接来源关系、
    /// 不接第二份可伪造的地址投影。占用必须是本 Nest 的（否则 `NoRegion`），
    /// 未发布的失败退回它的属主，已发布的失败进清理链。
    pub(crate) fn attach_shaped(
        self: &Arc<Self>,
        span: Arc<Span>,
        source: crate::basis::flat::FlatRange,
        active: &[ActiveRange],
    ) -> Result<Arc<Span>, NestError> {
        if let Err(error) = self.check_owner(&span) {
            return Err(self.abandon(span, error));
        }
        let mut ranges = Vec::new();
        if ranges.try_reserve(active.len()).is_err() {
            return Err(self.abandon(span, NestError::OutOfMemory));
        }
        ranges.extend_from_slice(active);
        if let Err(error) = span.install(source, ranges) {
            return Err(self.abandon(span, error));
        }
        match self.commit(&span) {
            Ok(()) => Ok(span),
            Err(error) => {
                self.settle_failed_install(&span);
                Err(error)
            }
        }
    }

    /// 一笔尚未发布的占用的归还方向：它的属主自己的表，不是接收调用的那一方。
    ///
    /// 占用身份是全局唯一的，所以“拿裸坐标去目标表里找一笔”不会误删别人的槽；
    /// 但那样也退不掉真属主的这一笔，因此归还一律按属主。
    /// 占用已经交出、本笔操作不再有承接者时唯一的归还路径：按属主交还并交出错误。
    ///
    /// 与调用方主动退的 `release_span` 成对；`give_back` / `give_back_span`
    /// 只是“退还”的原语，不包含“这笔操作失败”的语义。
    fn abandon(&self, span: Arc<Span>, error: NestError) -> NestError {
        self.give_back(&span);
        error
    }

    /// 安装失败的责任划分：已发布就把回收交给清理链（占用仍禁复用），
    /// 未发布就退回占用与临时数据。
    fn settle_failed_install(&self, mapping: &Arc<Span>) {
        let address = self.address.lock();
        let published = address
            .index
            .find(mapping.initial_start().as_usize())
            .is_some_and(|held| Arc::ptr_eq(&held, mapping));
        if !published {
            self.give_back(mapping);
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
        let start = mapping.range.start();
        let end = mapping.range.last();
        {
            let layout = self.address.lock();
            if !layout.occupancy.has_id(mapping.identity()) {
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
        let mut address = self.address.lock();
        let mut state = self.ready_state()?;
        if address.index.first_overlap(start.as_usize(), end).is_some() {
            return Err(NestError::AlreadyMapped);
        }
        state
            .retirement
            .mappings
            .try_reserve(1)
            .map_err(|_| NestError::OutOfMemory)?;
        mapping.state.lock().memos.reserve(published.len())?;
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
        state.retirement.mappings.push(Arc::downgrade(mapping));
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
        {
            let mut inner = mapping.state.lock();
            for (page, cell) in published {
                inner.memos.publish(page, cell);
            }
        }
        address.index.insert(node);
        state.retirement.mark_dirty(mapping.is_global());
        // Installation still owns its unpublished Span. Reserve the sole fence
        // initiator before unlocking, so an observer cannot steal progress and
        // make the installer discard a successfully committed mapping on Busy.
        let target = state.retirement.begin(self.id.get(), self.asid())?;
        drop(state);
        drop(lease);
        self.invalidate(target)
    }

    /// 把该窗口里已经物化/借用的页发布到本关系：不动 Flat 的物化。
    ///
    /// 只发布活动形状内的页，不偷分配未物化的页。
    fn publish(self: &Arc<Self>, selection: NestRange) -> Result<(), NestError> {
        let mapping = selection.span().clone();
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
        let mut inner = mapping.state.lock();
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
            if inner.memos.publication(page).is_none() {
                let address = mapping.range.start() + page * PAGE_SIZE;
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
        inner.memos.reserve(fresh.len())?;
        let source = inner.source.as_ref().ok_or(NestError::Missing)?;
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
        state.retirement.mappings.push(Arc::downgrade(mapping));
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
            inner.memos.publish(page, cell);
        }
        state.retirement.mark_dirty(mapping.is_global());
        drop(inner);
        drop(state);
        drop(lease);
        self.finish_invalidation()
    }

    /// Persistent source lookup never materializes or retains a physical Page.
    pub(crate) fn unpublish_page(
        &self,
        mapping: &Arc<Span>,
        address: VirtAddr,
        expected: &Arc<Page>,
    ) -> Result<(), NestError> {
        self.unpublish_page_with(mapping, address, expected, false)
    }

    fn unpublish_page_with(
        &self,
        mapping: &Arc<Span>,
        address: VirtAddr,
        expected: &Arc<Page>,
        polling: bool,
    ) -> Result<(), NestError> {
        if self.asid().is_kernel() || mapping.is_global() {
            return Err(NestError::Pinned);
        }
        assert!(
            address.offset() == 0,
            "unpublish_page takes a page-aligned address"
        );
        assert!(
            mapping.identity.belongs_to(self.id),
            "unpublish_page requires the mapping's own live nest"
        );
        self.finish_invalidation_with(polling)?;

        let mut state = self.state.lock();
        if !state.retirement.invalidation().is_clean() {
            return Err(NestError::Busy);
        }
        if !state.unpublish_page(self.geometry(), mapping, address, expected)? {
            return Ok(());
        }
        let target = state.retirement.begin(self.id.get(), self.asid())?;
        drop(state);
        // Release retained resources only after this exact invocation completes.
        self.start_invalidation(target, polling)?;
        mapping.finish_memo(mapping.page_index(address));
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
        let mapping = selection.span().clone();
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
        let mapping = selection.span();
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
                spans.push((
                    crate::metal::mmu::internal::canonical(start),
                    last - start + 1,
                ));
            }
        }
        Ok(spans)
    }

    fn span_key(&self, mapping: &Span) -> WakeKey {
        WakeKey::Span {
            space: self.id,
            span: mapping.identity(),
        }
    }

    pub(crate) fn detach(self: &Arc<Self>, selection: NestRange) -> Result<(), NestError> {
        let key = self.span_key(selection.span());
        let result = self.detach_range(selection);
        sign::signal(key);
        result
    }

    /// 按选择撤下：先退休该段占用，再撤 PTE，同步成功后只回收这一段。
    ///
    /// 失败时退休票、Page、表页、来源与待撤的活动全都保留，VA 禁复用；
    /// 沿用同一身份重试（再次 `detach`，或 `retry_invalidation`）才继续。
    fn detach_range(self: &Arc<Self>, selection: NestRange) -> Result<(), NestError> {
        let mapping = selection.span().clone();
        self.check_owner(&mapping)?;
        if self.is_closed() {
            return Err(NestError::NoRegion);
        }
        if self.has_pending_invalidation() {
            // 先把上一次失败的同步结算掉，再动这一段。
            self.finish_invalidation()?;
        }
        for (start, len) in self.owed_spans(&selection)? {
            if let Err(error) = self.withdraw(&mapping, start, len) {
                // A peer retry may claim/reclaim the exact settled ticket after
                // owed_spans took its snapshot. Whole-owner cleanup is already
                // complete in that case; strict Pages still rejects a hole.
                if error == NestError::NoRegion
                    && matches!(selection, NestRange::Whole(_))
                    && self.settled(mapping.identity())
                {
                    return Ok(());
                }
                return Err(error);
            }
        }
        Ok(())
    }

    /// 选择在当前状态下还欠着的段（活动 ∪ 未结算退休），按地址升序。
    ///
    /// `Pages` 还要求整个窗口都被欠着：选到已经结算完的页就是失败。
    /// 整段只取还欠着的那几段，因此部分撤下之后不会去碰已经没了的洞。
    fn owed_spans(&self, selection: &NestRange) -> Result<Vec<(VirtAddr, usize)>, NestError> {
        let mapping = selection.span();
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
            let layout = self.address.lock();
            for (start, last, _) in layout.occupancy.retired_of(mapping.identity())? {
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
            spans.push((
                crate::metal::mmu::internal::canonical(floor),
                last - floor + 1,
            ));
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
        let mut layout = self.address.lock();
        let prepared =
            layout
                .occupancy
                .prepare_retirement(start.as_usize(), last, mapping.identity())?;
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
        let last_page = mapping.page_index(crate::metal::mmu::internal::canonical(last));
        let removed = inner
            .memos
            .published()
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
            .reserve(prepared.len())
            .map_err(|_| NestError::OutOfMemory)?;
        state.prepare_withdrawal(start, len, self.geometry())?;

        // 开票之后没有可失败准备：票、PTE 与关系在同一提交内转入退休责任。
        let tickets = layout.occupancy.retire(prepared);
        state.withdraw_prepared(start, len, self.geometry());
        state.retirement.mappings.push(Arc::downgrade(mapping));
        if inner
            .memos
            .withdraw(first_page, last_page, &mut state.retirement.cells)
        {
            mapping.changed();
        }
        inner.active = next;
        drop(inner);
        layout.index.remove(mapping);
        if let Some(node) = next_node {
            layout.index.insert(node);
        }
        for &(_, _, ticket) in &tickets {
            state.retirement.tickets.retire(ticket);
        }
        state.retirement.mark_dirty(mapping.is_global());
        let target = state.retirement.begin(self.id.get(), self.asid())?;
        drop(state);
        drop(layout);
        // 三、同步成功之后才结算这一段的票；失败时票、Page、表页与来源都留着。
        self.invalidate(target)?;
        if !tickets.is_empty() {
            let mut layout = self.address.lock();
            let mut state = self.state.lock();
            for (_, _, ticket) in tickets {
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
        let key = self.span_key(&mapping);
        let result = (|| {
            self.check_owner(&*mapping)?;
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

    /// 把一段尚未挂载的地址占用分成两笔 Span；失败时释放这段占用。
    pub(crate) fn split(
        self: &Arc<Self>,
        span: Arc<Span>,
        left_size: usize,
    ) -> Result<(Arc<Span>, Arc<Span>), NestError> {
        self.check_owner(&span)?;
        let unpublished = {
            let state = span.state.lock();
            state.active.is_empty() && state.source.is_none()
        };
        if !unpublished {
            return Err(NestError::Busy);
        }
        let start = span.initial_start();
        let size = span.initial_size();
        let last = span.range.last();
        let level = span.level();
        let attrs = span.attrs;
        let original = span.identity();
        if left_size == 0 || left_size >= size || !left_size.is_multiple_of(PAGE_SIZE) {
            return Err(NestError::NotAligned);
        }
        let (right_start, left_id, right_id) = {
            let mut layout = self.address.lock();
            match layout
                .occupancy
                .split(start.as_usize(), last, original, left_size)
            {
                Ok(split) => split,
                Err(error) => {
                    drop(layout);
                    self.give_back(&span);
                    return Err(error.into());
                }
            }
        };
        let left_range = match SpanRange::new(start, left_size, level) {
            Ok(range) => range,
            Err(error) => {
                return Err(self.abort_split(start, left_size, last, left_id, right_id, error));
            }
        };
        let right = if self.cpu.is_some() {
            crate::metal::mmu::internal::canonical(right_start)
        } else {
            VirtAddr::new(right_start)
        };
        let right_range = match SpanRange::new(right, size - left_size, level) {
            Ok(range) => range,
            Err(error) => {
                return Err(self.abort_split(start, left_size, last, left_id, right_id, error));
            }
        };
        let left = match Span::vacant(self, SpanIdentity::new(left_id, self), left_range, attrs) {
            Ok(left) => left,
            Err(error) => {
                return Err(self.abort_split(start, left_size, last, left_id, right_id, error));
            }
        };
        let right_span =
            match Span::vacant(self, SpanIdentity::new(right_id, self), right_range, attrs) {
                Ok(right) => right,
                Err(error) => {
                    self.give_back_span(start, left_size, left_id);
                    return Err(self.abort_split(start, left_size, last, left_id, right_id, error));
                }
            };
        Ok((left, right_span))
    }

    fn abort_split(
        &self,
        start: VirtAddr,
        left_size: usize,
        last: usize,
        left_id: usize,
        right_id: usize,
        error: NestError,
    ) -> NestError {
        let mut layout = self.address.lock();
        let left_last = start.as_usize() + (left_size - 1);
        let _ = layout
            .occupancy
            .release(start.as_usize(), left_last, left_id);
        let _ = layout.occupancy.release(left_last + 1, last, right_id);
        error
    }

    /// 交还一笔尚未挂载的 Span 占用。
    pub(crate) fn release_span(&self, span: &Arc<Span>) -> Result<(), NestError> {
        self.check_span(span)?;
        if self.overlaps(span.initial_start(), span.initial_size())? {
            return Err(NestError::AlreadyMapped);
        }
        if self.give_back(span) {
            Ok(())
        } else {
            Err(NestError::NoRegion)
        }
    }

    fn give_back(&self, span: &Span) -> bool {
        self.give_back_span(span.initial_start(), span.initial_size(), span.identity())
    }

    /// 按占用身份交还一段尚未落地的跨度（凭证已经不在手里时用）。
    fn give_back_span(&self, start: VirtAddr, size: usize, identity: usize) -> bool {
        self.address.lock().occupancy.release(
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
            let mut layout = self.address.lock();
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
        self.finish_invalidation_with(false)
    }

    fn finish_invalidation_with(&self, polling: bool) -> Result<(), NestError> {
        let mut state = self.state.lock();
        if state.retirement.invalidation().is_clean() {
            return Ok(());
        }
        if let Some(round) = state.retirement.deferred() {
            // Service is lockless, one poll, with no cross-Hart waiting.
            let proof = round.poll()?;
            let phase = state.phase;
            let fenced = state.settle(proof)?;
            if phase != state.phase {
                self.changed();
            }
            drop(state);
            fenced.release();
            return Ok(());
        }
        if self.cpu.is_none() {
            return Err(NestError::NoRegion);
        }
        let target = state.retirement.begin(self.id.get(), self.asid())?;
        drop(state);
        self.start_invalidation(target, polling)
    }

    fn start_invalidation(
        &self,
        target: fence::Request<Asid>,
        polling: bool,
    ) -> Result<(), NestError> {
        if !polling {
            return self.invalidate(target);
        }
        let cpu = self.cpu.as_ref().ok_or(NestError::NoRegion)?;
        // The Fencing state already reserves this exact initiator. No COMMIT
        // or Nest lock crosses round submission; peers retain Busy until parked.
        let round = cpu.domain.defer(target);
        let mut state = self.state.lock();
        match round {
            Ok(round) => {
                state.retirement.park(round)?;
                Err(NestError::Busy)
            }
            Err(error) => {
                state.retirement.fail();
                Err(error)
            }
        }
    }

    /// The request's initiator owns this one hardware invocation through its
    /// checked completion. Observers cannot replace or abandon this request.
    fn invalidate(&self, target: fence::Request<Asid>) -> Result<(), NestError> {
        let cpu = self.cpu.as_ref().expect("CPU invalidation initiator");
        #[cfg(debug_assertions)]
        crate::basis::assert_unlocked();
        let result = fence::invalidate(&cpu.domain, target);
        let fenced = {
            let mut state = self.state.lock();
            match result {
                Ok(proof) => {
                    let phase = state.phase;
                    let fenced = state.settle(proof)?;
                    if phase != state.phase {
                        self.changed();
                    }
                    fenced
                }
                Err(_) => {
                    state.retirement.fail();
                    return Err(NestError::Shootdown);
                }
            }
        };
        // Retired Page and table resources can leave the lock. Address
        // tickets are reclaimed by detach or explicit retry_invalidation.
        fenced.release();
        Ok(())
    }

    /// 撤下全部关系并等算 shootdown；内核地址空间为进程期，不在此列。
    fn close(&self) -> Result<(), NestError> {
        if self.cpu.is_none() {
            return Err(NestError::Busy);
        }
        if self.asid().is_kernel() {
            return Err(NestError::Pinned);
        }
        self.finish_invalidation()?;
        let mut address = self.address.lock();
        let mut state = {
            let state = self.state.lock();
            if state.phase.is_closed() {
                drop(state);
                self.admission.closed();
                self.drain_closed();
                return Ok(());
            }
            if !state.retirement.invalidation().is_clean() {
                return Err(NestError::Busy);
            }
            state
        };
        let phase = state.phase;
        let result = state.close_mappings(&mut address.index, self.geometry());
        if phase != state.phase {
            self.changed();
        }
        drop(state);
        fault::wake_nest(self);
        result?;
        self.finish_invalidation()?;
        self.drain_closed();
        if self.is_closed() {
            self.admission.closed();
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
            self.drain_span_resources()?;
            if self.pending_cleanup.lock().is_some() {
                return Err(NestError::Busy);
            }
            if !sign::wipe_space(self.id) {
                return Err(NestError::Busy);
            }
            if !self.reap_spans() {
                return Err(NestError::Busy);
            }
            self.release_asid()?;
        }
        self.reap_spans();
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
    fn ensure_owned(&self, mapping: &Arc<Span>, _state: &NestState) -> Result<(), NestError> {
        if !self.admission.is_open() || !mapping.identity.belongs_to(self.id) {
            return Err(NestError::NoRegion);
        }
        Ok(())
    }

    fn ensure_span(&self, mapping: &Arc<Span>) -> Result<(), NestError> {
        self.check_owner(mapping)?;
        if self
            .spans
            .registered(mapping.identity())
            .and_then(|entry| entry.span)
            .is_none_or(|held| !Arc::ptr_eq(&held, mapping))
        {
            return Err(NestError::NoRegion);
        }
        Ok(())
    }

    fn check_owner(&self, span: &Span) -> Result<(), NestError> {
        if !span.identity.belongs_to(self.id) {
            return Err(NestError::NoRegion);
        }
        Ok(())
    }

    fn check_span(&self, span: &Span) -> Result<(), NestError> {
        self.check_owner(span)?;
        if !self.address.lock().occupancy.has_id(span.identity()) {
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
        let index = self.address.lock();
        let state = self.ready_state().ok()?;
        index.index.find(address.as_usize())?;
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
        let index = self.address.lock();
        let state = self.state.lock();
        FaultOutcome::probe(&state, &index.index, self.geometry(), address, kind)
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
            let address_index = self.address.lock();
            let mapping = address_index
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
        let index = self.address.lock();
        self.state.lock().audit(&index.index, self, self.geometry());
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
