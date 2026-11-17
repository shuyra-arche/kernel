//! An occupied address span and its range policy. Positions live in the Span's Memo table.
mod access;
mod geometry;
mod operations;
pub(crate) use geometry::SpanRange;
use super::super::index::source;
use super::super::memo::publication::{CellRuns, Publication};
use super::super::{CleanupNode, Nest, NestId};
use super::NestRange;
pub use access::{Access, ActiveRange};
use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::basis::flat::FlatRange;
use crate::basis::nest::NestError;
use crate::basis::sync::LegacyLock;
use crate::metal::generic::mmu::internal::Attributes;
use kernel_api::address::PAGE_SIZE;
use kernel_api::address::PageCount;
use kernel_api::address::PageOffset;
use kernel_api::address::VirtAddr;
use kernel_api::model::cell::CellLevel;

/// A stable address-span identity: the occupancy generation and its Nest.
///
/// The numeric identity is the ABI `SpanId`; the `NestId` attributes it without
/// retaining or locating the parent. It is descriptive and never grants access.
pub(crate) struct SpanIdentity {
    id: usize,
    nest: NestId,
}

impl SpanIdentity {
    pub(crate) fn new(id: usize, nest: &Arc<Nest>) -> Self {
        Self {
            id,
            nest: nest.id(),
        }
    }

    pub(crate) fn id(&self) -> usize {
        self.id
    }

    pub(crate) fn nest_id(&self) -> NestId {
        self.nest
    }

    /// Whether this span belongs to `nest`, without locating it.
    pub(crate) fn belongs_to(&self, nest: NestId) -> bool {
        self.nest == nest
    }
}

/// An occupied address span: identity, original range, permissions and source
/// selection.
///
/// It owns its per-position Memos and holds the shared Translation; the Nest
/// owns admission and address organization. Keeping a historical span cannot
/// keep a deleted position's Page or freeze alive.
pub struct Span {
    pub(crate) faults:
        LegacyLock<crate::basis::sync::order::FaultPolicy, super::super::fault::State>,
    pub(crate) identity: SpanIdentity,
    /// Shared Nest admission; final acceptance still happens under commit.
    admission: Arc<super::super::admission::Admission>,
    /// Shared hardware translation; Address organization stays in the Nest.
    pub(in crate::basis::nest) translation: Arc<super::super::translation::Translation>,
    pub(crate) range: SpanRange,
    range_id: core::sync::atomic::AtomicUsize,
    pub(crate) attrs: Attributes,
    pub(crate) state: LegacyLock<crate::basis::sync::order::Span, SpanState>,
    operation: core::sync::atomic::AtomicBool,
    revision: core::sync::atomic::AtomicU64,
    /// Weak discovery of sources exported through this span. Live permission
    /// remains in each Source's original Pend lineage.
    pub(crate) exports: LegacyLock<
        crate::basis::sync::order::SpanExports,
        crate::basis::pend::Tracking<crate::basis::pend::SourceAuthorization>,
    >,
}

impl core::fmt::Debug for Span {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Span")
            .field("initial_start", &self.initial_start())
            .field("initial_size", &self.initial_size())
            .finish_non_exhaustive()
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        // 关系退场：来源↔关系的关联里不留死项；最后一条走了就把容量也交回。
        source::forget();
    }
}

impl Span {
    pub(crate) fn bind_authority(&self, range: kernel_api::model::identity::PendId) {
        self.range_id
            .store(range.get(), core::sync::atomic::Ordering::Release);
    }

    pub(crate) fn authority(&self) -> kernel_api::model::identity::PendId {
        kernel_api::model::identity::PendId::mint(
            self.range_id.load(core::sync::atomic::Ordering::Acquire),
        )
    }

    pub(crate) fn version(&self) -> kernel_api::model::nest::SpanVersion {
        kernel_api::model::nest::SpanVersion::new(
            self.revision.load(core::sync::atomic::Ordering::Acquire),
        )
        .expect("span revision")
    }
    pub(in crate::basis::nest) fn prepare_revision(&self) -> Result<(), NestError> {
        self.version()
            .next()
            .ok_or(NestError::OutOfMemory)
            .map(|_| ())
    }
    /// Forced cleanup may exhaust a revision but never wrap or reopen admission.
    pub(in crate::basis::nest) fn changed(&self) {
        let _ = self.revision.try_update(
            core::sync::atomic::Ordering::AcqRel,
            core::sync::atomic::Ordering::Acquire,
            |v| v.checked_add(1),
        );
    }

    /// Occupy `range` without a source or active access. This is a legal
    /// address span; association and publication can follow independently.
    pub(in crate::basis::nest) fn vacant(
        nest: &Arc<Nest>,
        identity: SpanIdentity,
        range: SpanRange,
        attrs: Attributes,
    ) -> Result<Arc<Self>, NestError> {
        let (start, size, id) = (range.start(), range.size(), identity.id());
        if !Self::attrs_are_architectural(attrs) {
            nest.give_back_span(start, size, id);
            return Err(NestError::NotAligned);
        }
        match Arc::try_new(Self {
            faults: LegacyLock::new(super::super::fault::State::new()),
            identity,
            range_id: core::sync::atomic::AtomicUsize::new(0),
            admission: nest.admission.clone(),
            translation: (*nest.state).clone(),
            range,
            attrs,
            state: LegacyLock::new(SpanState::vacant()),
            operation: core::sync::atomic::AtomicBool::new(false),
            revision: core::sync::atomic::AtomicU64::new(1),
            exports: LegacyLock::new(crate::basis::pend::Tracking::new()),
        }) {
            Ok(span) => Ok(span),
            Err(_) => {
                nest.give_back_span(start, size, id);
                Err(NestError::OutOfMemory)
            }
        }
    }

    /// Admit the active access shape of an unpublished span. The source stays
    /// absent; association publishes a logical source separately.
    pub(in crate::basis::nest) fn stage(&self, active: Vec<ActiveRange>) -> Result<(), NestError> {
        self.check_shape(&active, None)?;
        let mut state = self.state.lock();
        if !state.active.is_empty() || state.source.is_some() {
            return Err(NestError::Busy);
        }
        state.initial_access = active
            .iter()
            .fold(Access::all(), |access, range| access & range.ceiling);
        state.active = active;
        Ok(())
    }

    /// Attach the implicit source and the full active shape of this span.
    pub(in crate::basis::nest) fn install(
        &self,
        source: FlatRange,
        active: Vec<ActiveRange>,
    ) -> Result<(), NestError> {
        self.check_shape(&active, Some(&source))?;
        let mut state = self.state.lock();
        if !state.active.is_empty() || state.source.is_some() {
            return Err(NestError::Busy);
        }
        state.initial_access = active
            .iter()
            .fold(Access::all(), |access, range| access & range.ceiling);
        state.active = active;
        state.source = Some(source);
        Ok(())
    }

    fn check_shape(
        &self,
        active: &[ActiveRange],
        source: Option<&FlatRange>,
    ) -> Result<(), NestError> {
        let cell_size = self.range.level().bytes().ok_or(NestError::NotAligned)?;
        let start = self.range.start().as_usize();
        let end = self.range.last();
        if active.is_empty()
            || source.is_some_and(|source| {
                source.level() != self.range.level()
                    || source.page_count().checked_bytes() != Some(self.range.size())
            })
        {
            return Err(NestError::NotAligned);
        }
        for range in active {
            let last = range.end();
            if range.start.as_usize() < start
                || last > end
                || !(range.start.as_usize() - start).is_multiple_of(cell_size)
                || !(last - range.start.as_usize() + 1).is_multiple_of(cell_size)
            {
                return Err(NestError::NoRegion);
            }
        }
        if active
            .windows(2)
            .any(|pair| pair[0].end() >= pair[1].start.as_usize())
        {
            return Err(NestError::AlreadyMapped);
        }
        Ok(())
    }

    pub(in crate::basis::nest) fn prepare_cleanup(&self) -> Result<(), NestError> {
        let mut state = self.state.lock();
        if state.cleanup.is_none() {
            state.cleanup =
                Some(Box::try_new(CleanupNode::empty()).map_err(|_| NestError::OutOfMemory)?);
        }
        Ok(())
    }

    pub(in crate::basis::nest) fn take_cleanup(&self) -> Option<Box<CleanupNode>> {
        self.state.lock().cleanup.take()
    }

    pub fn initial_start(&self) -> VirtAddr {
        self.range.start()
    }

    pub fn initial_size(&self) -> usize {
        self.range.size()
    }

    pub fn initial_range(&self) -> (VirtAddr, usize) {
        (self.initial_start(), self.initial_size())
    }

    /// 这段映射在占用表里的身份（generation）；ABI 的 `SpanId` 就是它。
    pub(crate) fn identity(&self) -> usize {
        self.identity.id()
    }

    /// 这段关系等待位的那一对坐标：`(软件空间身份, 关系身份)`。
    ///
    /// 用**原始** Nest（关闭了也照给）：放行只看 NestId 与身份，已关闭的空间里
    /// 没人等在这个键上，`owner()` 的“已关闭即无主”语义在这里反而会漏掉
    /// 刚被撤下的那一段。只交出两个标量，不留 Nest 强引用。
    pub(crate) fn wake_key(&self) -> Option<(NestId, usize)> {
        Some((self.identity.nest_id(), self.identity()))
    }

    /// 整段选择：操作时按当前活动与未结算退休解析。
    pub fn whole(self: &Arc<Self>) -> NestRange {
        NestRange::Whole(self.clone())
    }

    /// 初始范围内的固定页坐标选择；越界即拒。
    pub fn range(
        self: &Arc<Self>,
        offset: PageOffset,
        count: PageCount,
    ) -> Result<NestRange, NestError> {
        NestRange::pages(self, offset, count)
    }

    pub(crate) fn source(&self) -> Option<FlatRange> {
        self.state.lock().source.clone()
    }

    pub(crate) fn level(&self) -> CellLevel {
        self.range.level()
    }

    pub(crate) fn grain(&self) -> usize {
        self.range.level().pages().expect("admitted Cell geometry")
    }

    pub(crate) fn access(&self, page: usize) -> Result<(Access, Access), NestError> {
        let address = self.position(page)?;
        let last = address
            .checked_add(self.grain() * PAGE_SIZE - 1)
            .ok_or(NestError::NoRegion)?;
        let inner = self.state.lock();
        let range = inner.range_at(address).ok_or(NestError::NoRegion)?;
        if range.end() < last {
            return Err(NestError::NoRegion);
        }
        Ok((range.ceiling, range.current))
    }

    /// Original geometry remains valid after the active address has retired.
    pub(in crate::basis::nest) fn position(&self, page: usize) -> Result<usize, NestError> {
        let grain = self.grain();
        if !page.is_multiple_of(grain) {
            return Err(NestError::NotAligned);
        }
        let end = page.checked_add(grain).ok_or(NestError::NoRegion)?;
        if end > self.initial_size() / PAGE_SIZE {
            return Err(NestError::NoRegion);
        }
        self.initial_start()
            .get()
            .checked_add(page.checked_mul(PAGE_SIZE).ok_or(NestError::NoRegion)?)
            .ok_or(NestError::NoRegion)
    }

    pub(crate) fn is_open(&self) -> bool {
        self.admission.is_open() && self.state.lock().phase == SpanPhase::Open
    }

    pub(in crate::basis::nest) fn close(&self) {
        let mut inner = self.state.lock();
        if inner.phase == SpanPhase::Open {
            inner.phase = SpanPhase::Closing;
            inner.drain_cursor = None;
            self.changed();
            self.faults.lock().close();
        }
    }

    /// 一段当前权限对应的叶子 PTE 位：V、RWX 与整段关系的 U/G/A/D。
    pub(crate) fn leaf_attributes(&self, current: Access) -> Attributes {
        current.to_attributes() | self.attrs
    }

    /// 架构属性只含 U/G/A/D；给出其它位就拒。
    pub(crate) fn attrs_are_architectural(attrs: Attributes) -> bool {
        !attrs.intersects(Attributes::READ | Attributes::WRITE | Attributes::EXECUTE)
    }

    /// 这段关系的叶子是不是全局条目。
    ///
    /// 全局条目对 ASID 定向的 `sfence.vma` 免疫，所以“按本关系自己的 ASID 兑失效”
    /// 不能作为它已经不再压着某个物理页的证据。
    pub(crate) fn is_global(&self) -> bool {
        self.attrs.contains(Attributes::GLOBAL)
    }

    /// `address` 相对初始范围的页号；调用方保证它在初始范围内。
    /// 活动区间的只读快照（debug 审计用：跨关系比较不能套着关系锁）。
    #[cfg(debug_assertions)]
    pub(crate) fn active_snapshot(&self) -> Vec<(usize, usize)> {
        self.state
            .lock()
            .active
            .iter()
            .map(|range| (range.start.as_usize(), range.end()))
            .collect()
    }

    pub(crate) fn page_index(&self, address: VirtAddr) -> usize {
        (address.as_usize() - self.initial_start().as_usize()) / PAGE_SIZE
    }

    /// 活动区间是否与 [lo, hi] 相交。
    pub(crate) fn overlaps(&self, lo: usize, hi: usize) -> bool {
        self.state
            .lock()
            .active
            .iter()
            .any(|range| range.overlaps(lo, hi))
    }

    pub(crate) fn operation(&self) -> Option<RangeOperation<'_>> {
        self.operation
            .compare_exchange(
                false,
                true,
                core::sync::atomic::Ordering::AcqRel,
                core::sync::atomic::Ordering::Acquire,
            )
            .ok()
            .map(|_| RangeOperation(&self.operation))
    }
    pub(crate) fn inspect_memo(
        &self,
        index: usize,
    ) -> Result<crate::basis::nest::memo::MemoInfo, NestError> {
        let state = self.state.lock();
        if state.phase == SpanPhase::Gone {
            return Err(NestError::NoRegion);
        }
        let address = self.position(index)?;
        let vacant = crate::basis::nest::memo::Memo::vacant();
        let mut info = state
            .memos
            .get(index)
            .unwrap_or(&vacant)
            .inspect(VirtAddr::new(address));
        info.level = self.level();
        Ok(info)
    }
}

pub(crate) struct RangeOperation<'a>(&'a core::sync::atomic::AtomicBool);
impl Drop for RangeOperation<'_> {
    fn drop(&mut self) {
        self.0.store(false, core::sync::atomic::Ordering::Release);
    }
}

// 映射改权的纯决策：只读取逻辑状态，返回新活动区间和页表更新意图。
// Nest 在同一持锁区间内准备硬件资源、提交并结算失效；这里不获取锁或操作页表。

#[derive(Clone, Copy)]
pub(crate) enum Protection {
    SetAccess(Access),
}

pub(crate) struct PermissionUpdate {
    pub address: VirtAddr,
    pub access: Access,
}

pub(crate) struct ProtectionTransition {
    pub active: Vec<ActiveRange>,
    pub updates: Vec<PermissionUpdate>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
/// One bounded step of a Span-local close drain.
pub(in crate::basis::nest) enum DrainStep {
    Done,
    Progress,
    Blocked,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::basis::nest) enum SpanPhase {
    Open,
    Closing,
    Gone,
}

/// Range policy and optional implicit source; positions are owned by Nest.
pub(crate) struct SpanState {
    // Reserved before publication so failed withdrawal can queue the Span
    // without allocating or making its address reusable.
    pub(in crate::basis::nest) cleanup: Option<Box<CleanupNode>>,
    pub(crate) active: Vec<ActiveRange>,
    pub(crate) source: Option<FlatRange>,
    /// Logical positions and their exact hardware obligations, owned here.
    pub(in crate::basis::nest) memos: crate::basis::nest::memo::Memos,
    /// The narrowest creation access across all active ranges.
    pub(crate) initial_access: Access,
    pub(in crate::basis::nest) drain_cursor: Option<usize>,
    pub(in crate::basis::nest) phase: SpanPhase,
}

impl SpanState {
    /// Remove a settled non-active position after the covering fence.
    pub(crate) fn take_deleted(&mut self, base: VirtAddr) -> Option<super::super::memo::Memo> {
        if self.source.is_none() {
            return None;
        }
        self.memos.take_deleted(base, &self.active)
    }

    pub(crate) fn vacant() -> Self {
        Self {
            cleanup: None,
            active: Vec::new(),
            source: None,
            memos: crate::basis::nest::memo::Memos::new(),
            initial_access: Access::empty(),
            drain_cursor: None,
            phase: SpanPhase::Open,
        }
    }

    /// 活动区间是否连续覆盖 [lo, hi]。
    pub(crate) fn covers(&self, lo: usize, hi: usize) -> bool {
        ActiveRange::cover(&self.active, lo, hi)
    }

    /// 覆盖 `address` 的活动区间。
    pub(crate) fn range_at(&self, address: usize) -> Option<&ActiveRange> {
        self.active.iter().find(|range| range.contains(address))
    }

    /// 帧分段：以 `base` 为虚拟起点、按活动区间 current 与物理连续性切段。
    pub(crate) fn runs<'a>(
        &'a self,
        base: kernel_api::address::VirtAddr,
        cells: &'a [(usize, Publication)],
    ) -> CellRuns<'a> {
        CellRuns::new(base, cells, &self.active)
    }

    /// `base` 与 `window` 来自同一 NestRange；窗口非空、页对齐且位于初始范围。
    /// 全程只读：拒绝或分配失败都不修改逻辑权限，也不发布任何硬件权限。
    pub(crate) fn protection_transition(
        &self,
        base: VirtAddr,
        (lo, hi): (usize, usize),
        request: Protection,
        publishes: impl Fn(usize) -> bool,
    ) -> Result<ProtectionTransition, NestError> {
        if let Protection::SetAccess(access) = request
            && !access.is_leaf_set()
        {
            return Err(NestError::NotAligned);
        }
        if !self.covers(lo, hi) {
            return Err(NestError::NoRegion);
        }
        let active = match request {
            Protection::SetAccess(access) => {
                if self
                    .active
                    .iter()
                    .filter(|range| range.overlaps(lo, hi))
                    .any(|range| !range.ceiling.contains(access))
                {
                    return Err(NestError::WidenDenied);
                }
                ActiveRange::split(&self.active, lo, hi, Some(access))?
            }
        };
        let first = (lo - base.as_usize()) / PAGE_SIZE;
        let pages = (hi - lo + 1) / PAGE_SIZE;
        let borrowed = self.source.as_ref().is_some_and(FlatRange::is_borrowed);
        let mut updates = Vec::new();
        updates
            .try_reserve(pages)
            .map_err(|_| NestError::OutOfMemory)?;
        for page in first..first + pages {
            let address = base + page * PAGE_SIZE;
            let access =
                ActiveRange::current_at(&active, address.as_usize()).ok_or(NestError::NoRegion)?;
            if borrowed || publishes(page) {
                updates.push(PermissionUpdate { address, access });
            }
        }
        Ok(ProtectionTransition { active, updates })
    }
}
