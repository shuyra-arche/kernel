//! A reserved address span and its range policy. Positions live in Nest's Memo table.
mod access;
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
use crate::basis::nest::Reservation;
use crate::metal::generic::mmu::Attributes;
use crate::metal::sync::Level;
use crate::metal::sync::SpinLock;
use kernel_api::address::PAGE_SIZE;
use kernel_api::address::PageCount;
use kernel_api::address::PageOffset;
use kernel_api::address::VirtAddr;
use kernel_api::model::cell::CellLevel;

/// An occupied address span: reservation, permissions and source selection.
///
/// It has no per-position Page, generation, publication or retirement state.
/// Nest owns those Memos. Keeping a historical span cannot keep a deleted
/// position's Page or freeze alive. The ABI ReservationId denotes this reservation.
/// Architectural U/G/A/D policy is fixed for the span; logical permissions may narrow.
pub struct Span {
    pub(crate) reservation: Reservation,
    pub(crate) attrs: Attributes,
    level: CellLevel,
    initial_access: Access,
    pub(crate) state: SpinLock<SpanState>,
    operation: core::sync::atomic::AtomicBool,
    revision: core::sync::atomic::AtomicU64,
    /// Weak discovery of sources exported through this reservation. Live
    /// permission remains in each Source's original Pend lineage.
    pub(crate) exports:
        SpinLock<crate::basis::pend::Tracking<crate::basis::pend::SourceAuthorization>>,
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
    pub(crate) fn initial_access(&self) -> Access {
        self.initial_access
    }
    pub(crate) fn version(&self) -> kernel_api::model::nest::ReservationVersion {
        kernel_api::model::nest::ReservationVersion::new(
            self.revision.load(core::sync::atomic::Ordering::Acquire),
        )
        .expect("reservation revision")
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

    /// 建立一段关系；活动区间必须落在初始范围内，来源跨度必须等长。
    ///
    /// 构造失败即退回占用：调用方交出的凭证不会随失败一起丢掉。
    pub(crate) fn new(
        reservation: Reservation,
        attrs: Attributes,
        active: Vec<ActiveRange>,
        level: CellLevel,
        source: Option<FlatRange>,
    ) -> Result<Arc<Self>, NestError> {
        let (start, size) = (reservation.start(), reservation.size());
        let end = start.as_usize().checked_add(size - 1);
        let reject = |error: NestError| -> NestError {
            // 构造失败即按属主退回占用：凭证不会随失败一起丢掉。
            Nest::give_back_unpublished(&reservation);
            error
        };
        let Some(end) = end else {
            return Err(reject(NestError::NotAligned));
        };
        let Some(cell_size) = level.bytes() else {
            return Err(reject(NestError::NotAligned));
        };
        if size == 0
            || !size.is_multiple_of(cell_size)
            || active.is_empty()
            || source.as_ref().is_some_and(|source| {
                source.level() != level || source.page_count().checked_bytes() != Some(size)
            })
        {
            return Err(reject(NestError::NotAligned));
        }
        for range in &active {
            let last = range.end();
            if range.start.as_usize() < start.as_usize()
                || last > end
                || !(range.start.as_usize() - start.as_usize()).is_multiple_of(cell_size)
                || !(last - range.start.as_usize() + 1).is_multiple_of(cell_size)
            {
                return Err(reject(NestError::NoRegion));
            }
        }
        if active
            .windows(2)
            .any(|pair| pair[0].end() >= pair[1].start.as_usize())
        {
            return Err(reject(NestError::AlreadyMapped));
        }
        let nest = reservation.nest();
        let identity = reservation.id();
        match Arc::try_new(Self {
            reservation,
            operation: core::sync::atomic::AtomicBool::new(false),
            revision: core::sync::atomic::AtomicU64::new(1),
            exports: SpinLock::new(crate::basis::pend::Tracking::new()),
            attrs,
            level,
            initial_access: active
                .iter()
                .fold(Access::all(), |access, range| access & range.ceiling),
            state: SpinLock::new_level(Level::Reservation, SpanState::new(active, source)),
        }) {
            Ok(mapping) => Ok(mapping),
            Err(_) => {
                if let Some(nest) = nest {
                    nest.give_back_span(start, size, identity);
                }
                Err(NestError::OutOfMemory)
            }
        }
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
        self.reservation.start()
    }

    pub fn initial_size(&self) -> usize {
        self.reservation.size()
    }

    pub fn initial_range(&self) -> (VirtAddr, usize) {
        (self.initial_start(), self.initial_size())
    }

    /// 这段映射在占用表里的身份（generation）；现有 ABI 的 `ReservationId` 就是它。
    pub(crate) fn identity(&self) -> usize {
        self.reservation.id()
    }

    /// 认住它的那个 Nest；已关闭的 Nest 不算。
    pub fn owner(&self) -> Option<Arc<Nest>> {
        let nest = self.reservation.nest()?;
        (!nest.is_closed()).then_some(nest)
    }

    /// 这段关系等待位的那一对坐标：`(软件空间身份, 关系身份)`。
    ///
    /// 用**原始** Nest（关闭了也照给）：放行只看 NestId 与身份，已关闭的空间里
    /// 没人等在这个键上，`owner()` 的“已关闭即无主”语义在这里反而会漏掉
    /// 刚被撤下的那一段。只交出两个标量，不留 Nest 强引用。
    pub(crate) fn wake_key(&self) -> Option<(NestId, usize)> {
        let nest = self.reservation.nest()?;
        Some((nest.id(), self.identity()))
    }

    /// 活动库存的快照。
    pub fn active(&self) -> Result<Vec<ActiveRange>, NestError> {
        let state = self.state.lock();
        let mut active = Vec::new();
        active
            .try_reserve(state.active.len())
            .map_err(|_| NestError::OutOfMemory)?;
        active.extend_from_slice(&state.active);
        Ok(active)
    }

    pub fn is_live(&self) -> bool {
        self.owner().is_some() && !self.state.lock().active.is_empty()
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
        self.level
    }

    pub(crate) fn grain(&self) -> usize {
        self.level.pages().expect("admitted Cell geometry")
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

    pub(in crate::basis::nest) fn is_open(&self) -> bool {
        self.state.lock().phase == ReservationPhase::Open
    }

    pub(in crate::basis::nest) fn close(&self) {
        let mut inner = self.state.lock();
        if inner.phase == ReservationPhase::Open {
            inner.phase = ReservationPhase::Closing;
            self.changed();
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
    ) -> Result<kernel_api::model::cell::CellInfo, NestError> {
        self.reservation
            .nest()
            .ok_or(NestError::NoRegion)?
            .inspect_memo(self, index)
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
    Narrow(Access),
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
pub(in crate::basis::nest) enum ReservationPhase {
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
    pub(in crate::basis::nest) phase: ReservationPhase,
}

impl SpanState {
    pub(crate) fn new(active: Vec<ActiveRange>, source: Option<FlatRange>) -> Self {
        Self {
            cleanup: None,
            active,
            source,
            phase: ReservationPhase::Open,
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
            Protection::Narrow(ceiling) => ActiveRange::narrow(&self.active, lo, hi, ceiling)?,
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
