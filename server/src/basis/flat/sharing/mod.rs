mod authority;
mod close;
mod mapping;

use alloc::sync::Arc;
use alloc::sync::Weak;
use alloc::vec::Vec;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering;

use crate::basis::flat::BorrowedRegion;
use crate::basis::flat::Flat;
use crate::basis::flat::FlatError;
use crate::basis::flat::FlatRange;
use crate::basis::nest::Access;
use crate::basis::nest::Nest;
use crate::basis::nest::NestError;
use crate::basis::nest::Placement;
use crate::basis::nest::RegionKind;
use crate::basis::nest::Span;
use crate::basis::pend::Pend;
use crate::basis::pend::SourceAuthorization;
use crate::basis::sign;
use crate::basis::sign::WakeKey;
use crate::basis::unit::anchor::Anchor;
use crate::metal::sync::Level;
use crate::metal::sync::SpinLock;
use kernel_api::abi::call::pend::PendFail;
use kernel_api::address::PAGE_SIZE;
use kernel_api::address::PageCount;
use kernel_api::address::PageOffset;
use kernel_api::address::PhysAddr;
use kernel_api::model::identity::PendId;
use kernel_api::model::identity::TaskId;
use kernel_api::model::modifiable::Modifiable;

use authority::Authority;
use mapping::HeldMap;

pub(crate) use close::close;
pub(crate) use close::pending_close;

/// 资源授权：一段经过授权的来源范围，它的授权记录与活跃关系。
///
/// 来源由 `source` 唯一承担：保留 RAM、设备区间与后续挂载都从这里出发，
/// 不为持有内存另建 kernel VA，也没有 keeper / canonical 来源视图。
/// 授权记录（token → 权限位）与实际 `Span` 分开保存；
/// 该 token 下的关系自带 token，所以撤销边不丢。
pub struct Sharing {
    source: FlatRange,
    /// 这份来源的授权记录：从已有来源创建时就是那一份（根 Pend 接手），
    /// 否则为 `None`。撤销时由它收回名下关系。
    authorization: SpinLock<Option<Arc<SourceAuthorization>>>,
    maps: SpinLock<Vec<HeldMap>>,
    authorities: SpinLock<Vec<Authority>>,
    live: AtomicBool,
    operating: AtomicBool,
    owner: TaskId,
    pub(crate) id: usize,
    life: Arc<Anchor>,
}

unsafe impl Send for Sharing {}
unsafe impl Sync for Sharing {}

impl Sharing {
    fn new(source: FlatRange, size: usize, owner: TaskId) -> Result<Arc<Self>, PendFail> {
        if source.page_count().checked_bytes() != Some(size) {
            return Err(PendFail::Denied);
        }
        let mut maps = Vec::new();
        maps.try_reserve_exact(1)
            .map_err(|_| PendFail::OutOfMemory)?;
        let mut authorities = Vec::new();
        authorities
            .try_reserve_exact(1)
            .map_err(|_| PendFail::OutOfMemory)?;
        Arc::try_new(Self {
            source,
            authorization: SpinLock::new(None),
            maps: SpinLock::new(maps),
            authorities: SpinLock::new_level(Level::L3, authorities),
            live: AtomicBool::new(true),
            operating: AtomicBool::new(false),
            owner,
            id: crate::basis::view::next_id()?,
            life: Anchor::try_new().map_err(|_| PendFail::OutOfMemory)?,
        })
        .map_err(|_| PendFail::OutOfMemory)
    }

    /// 从一份**完整来源范围**建立 Share：不造 keeper / canonical 来源视图，
    /// 真实关系由调用方（创建事务）决定什么时候登进来。
    pub(crate) fn from_source(
        source: FlatRange,
        size: usize,
        owner: TaskId,
    ) -> Result<Arc<Self>, PendFail> {
        if source.page_count().checked_bytes() != Some(size) {
            return Err(PendFail::Denied);
        }
        Self::new(source, size, owner)
    }

    pub(crate) fn owner(&self) -> TaskId {
        self.owner
    }
    /// 授权的来源范围。普通 clone 只是强持同一 `Flat`，不产生独立 Hold。
    pub(crate) fn source(&self) -> FlatRange {
        self.source.clone()
    }

    /// 这份 Share 的来源授权记录；旧路径建出来的 Share 没有。
    pub(crate) fn authorization(&self) -> Option<Arc<SourceAuthorization>> {
        self.authorization.lock().clone()
    }

    /// 把一份已有的来源授权接到这个 Share 上：根 Pend 接手，一处权威。
    pub(crate) fn attach_source(&self, authorization: &Arc<SourceAuthorization>) {
        *self.authorization.lock() = Some(authorization.clone());
    }
    pub(crate) fn life(&self) -> Weak<Anchor> {
        Arc::downgrade(&self.life)
    }
    pub(crate) fn alive(&self) -> bool {
        self.live.load(Ordering::Acquire)
    }

    /// 登记一份授权：首个授权同时接管输入映射，它成为该 token 下的活跃关系。
    ///
    /// 两个集合的容量先各自预留，之后不再失败；授权与放置由 Share 操作租约串行化。
    pub(crate) fn register_pend(
        &self,
        token: PendId,
        authority: Arc<Pend>,
    ) -> Result<(), PendFail> {
        {
            let mut authorities = self.authorities.lock();
            if authorities.len() >= 1024 {
                return Err(PendFail::OutOfMemory);
            }
            authorities
                .try_reserve(1)
                .map_err(|_| PendFail::OutOfMemory)?;
        }
        self.authorities
            .lock()
            .push(Authority::new(token, authority));
        Ok(())
    }

    pub(crate) fn release_pend(&self, token: PendId) {
        self.authorities
            .lock()
            .retain(|record| !record.authorizes(token));
    }

    pub(crate) fn authority(&self, token: PendId) -> Option<Arc<Pend>> {
        self.authorities
            .lock()
            .iter()
            .find(|record| record.authorizes(token))
            .map(Authority::authority)
    }

    pub(crate) fn reserve_records(&self, count: usize) -> Result<(), PendFail> {
        let mut maps = self.maps.lock();
        if count > 1024usize.saturating_sub(maps.len()) {
            return Err(PendFail::OutOfMemory);
        }
        maps.try_reserve(count).map_err(|_| PendFail::OutOfMemory)
    }

    /// 把一段已落地的关系记入该 token：先核对授权记录，再入活跃 Span 表。
    pub(crate) fn record(
        &self,
        token: PendId,
        space: &Arc<Nest>,
        mapping: Arc<Span>,
    ) -> Result<(), Arc<Span>> {
        let permitted = self
            .authorities
            .lock()
            .iter()
            .any(|record| record.authorizes(token) && record.permitted());
        if !self.alive() || !permitted {
            return Err(mapping);
        }
        self.maps
            .lock()
            .push(HeldMap::placed(mapping, space, token));
        Ok(())
    }

    pub(crate) fn pending_maps(&self, token: PendId) -> bool {
        self.maps
            .lock()
            .iter()
            .any(|entry| entry.holds(token) && !entry.placement.settled(entry.mapping.identity()))
    }
    pub(crate) fn has_maps(&self) -> bool {
        !self.maps.lock().is_empty()
    }

    pub(crate) fn close_maps(&self, token: Option<PendId>) -> Result<(), PendFail> {
        for _ in 0..4 {
            let entry = {
                let mut maps = self.maps.lock();
                let index = maps
                    .iter()
                    .position(|entry| token.is_none_or(|token| entry.holds(token)));
                match index {
                    Some(index) => maps.remove(index),
                    None => return Ok(()),
                }
            };
            {
                let space = &entry.placement;
                let id = entry.mapping.identity();
                let result = (|| {
                    if !space.settled(id) {
                        space.retry_invalidation()?;
                        if !space.settled(id) {
                            space.detach(entry.mapping.whole())?;
                        }
                    }
                    Ok::<_, NestError>(())
                })();
                if let Err(error) = result
                    && !space.settled(id)
                {
                    self.maps.lock().push(entry);
                    return Err(pend_map_error(error));
                }
                space.retire_reservation(id);
            }
        }
        if self
            .maps
            .lock()
            .iter()
            .any(|entry| token.is_none_or(|t| entry.holds(t)))
        {
            Err(PendFail::Busy)
        } else {
            Ok(())
        }
    }
}

/// 这条关系能不能代表**整份来源**被复用。
///
/// 来源窗口必须与 Share 整窗一致（同一 Flat 的同一段），活动区间必须从起点连续
/// 覆盖到末尾：Unseal 前允许同源多个 Attach（slice、中间有洞），那些关系仍归撤销
/// 清理，但不能顶替整窗去复用——否则会把 slice 的第 1 页当成源 0 页，或报整长
/// 却只有几个 PTE。
fn covers_source(source: &FlatRange, entry: &HeldMap) -> bool {
    let mapping = &entry.mapping;
    if mapping
        .source()
        .is_none_or(|mapped| !mapped.same_range(source))
    {
        return false;
    }
    let (start, size) = mapping.initial_range();
    let Ok(active) = mapping.active() else {
        return false;
    };
    let mut covered = 0usize;
    for range in active {
        if range.start.as_usize() != start.as_usize() + covered {
            return false;
        }
        covered += range.size.get();
    }
    covered == size
}

fn pend_map_error(error: NestError) -> PendFail {
    match error {
        NestError::OutOfMemory => PendFail::OutOfMemory,
        NestError::Shootdown | NestError::Busy => PendFail::Busy,
        _ => PendFail::Denied,
    }
}

/// 来源自己的失败码折成资源授权码，沿用地址空间路径的同一条折叠。
fn pend_source_error(error: FlatError) -> PendFail {
    pend_map_error(NestError::from(error))
}

pub(crate) struct SharingOperation(Arc<Sharing>);

impl SharingOperation {
    pub(crate) fn acquire(meta: &Arc<Sharing>) -> Option<Self> {
        meta.operating
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .ok()?;
        Some(Self(meta.clone()))
    }
}

impl Drop for SharingOperation {
    fn drop(&mut self) {
        self.0.operating.store(false, Ordering::Release);
    }
}

impl Drop for Sharing {
    fn drop(&mut self) {
        seal(self);
        let maps = core::mem::take(&mut *self.maps.lock());
        for entry in maps {
            let space = entry.placement;
            let id = entry.mapping.identity();
            if !space.settled(id) {
                let _ = space.dispose(entry.mapping);
            }
            space.retire_reservation(id);
        }
    }
}

/// 把一条关系的每个活动区间的上限与当前权限都收到 `access`。
///
/// 收窄是幂等的：已在同一上限上的区间重放不改变库存与 PTE。
fn narrow_mapping(space: &Arc<Nest>, mapping: &Arc<Span>, access: Access) -> Result<(), NestError> {
    for range in mapping.active()? {
        let offset = (range.start.as_usize() - mapping.initial_start().as_usize()) / PAGE_SIZE;
        let count = PageCount::new(range.size.get() / PAGE_SIZE).ok_or(NestError::NotAligned)?;
        space.narrow(mapping.range(PageOffset::new(offset), count)?, access)?;
    }
    Ok(())
}

pub(crate) fn open(
    meta: &Arc<Sharing>,
    token: PendId,
    space: &Arc<Nest>,
    access: Access,
) -> Result<(usize, usize), PendFail> {
    let _operation = SharingOperation::acquire(meta).ok_or(PendFail::Busy)?;
    if !meta.alive() {
        return Err(PendFail::Dead);
    }
    let authority = meta.authority(token).ok_or(PendFail::Denied)?;
    let modifiable = authority.modifiable().ok_or(PendFail::Dead)?;
    if !modifiable.allows(Modifiable::No)
        || access.contains(Access::WRITE) && modifiable != Modifiable::Yes
    {
        return Err(PendFail::Denied);
    }
    // 这一枚分支的有效窗口：有窗口就只映那一段，本域看到的与报出去的都只是它。
    let source = match authority.window() {
        Some((offset, count)) => meta
            .source()
            .range(offset, count)
            .map_err(|_| PendFail::Denied)?,
        None => meta.source(),
    };
    let size = source
        .page_count()
        .checked_bytes()
        .ok_or(PendFail::NotAligned)?;
    let authorization = meta.authorization();
    let window = authorization
        .as_ref()
        .map(|_| {
            let (offset, count) = authority
                .window()
                .unwrap_or((PageOffset::ZERO, meta.source().page_count()));
            crate::basis::pend::Geometry::new(offset, count)
        })
        .transpose()?;
    // The operation lease serializes Open with derivation, narrowing and
    // physical withdrawal. Source flight registration additionally prevents
    // retirement from settling this publication while the ledger closes.
    // Neither lease requires COMMIT across page-table work or remote fences.
    let commit = crate::basis::commit();
    if !meta.alive()
        || !authority.permits(kernel_api::model::pend::Operations::Share(
            kernel_api::model::pend::ShareRights::OPEN,
        ))
    {
        return Err(PendFail::Dead);
    }
    if let Some(authorization) = &authorization {
        let (offset, count) = authority
            .window()
            .unwrap_or((PageOffset::ZERO, meta.source().page_count()));
        authorization.check_publish(offset, count)?;
    }
    let reusable = {
        let maps = meta.maps.lock();
        maps.iter()
            .find(|entry| {
                entry.holds(token)
                    && entry.placed_in(space)
                    && entry.live()
                    && covers_source(&source, entry)
            })
            .map(|entry| entry.mapping.clone())
    };
    if let Some(mapping) = reusable {
        let id = mapping.identity();
        if let (Some(authorization), Some(window)) = (&authorization, &window) {
            authorization.enter_publish_range(id, window.clone(), Some(&authority))?;
        }
        drop(commit);
        let result = narrow_mapping(space, &mapping, access).map_err(pend_map_error);
        let permitted = {
            let _commit = crate::basis::commit();
            meta.alive() && authority.modifiable().is_some()
        };
        if let Some(authorization) = &authorization {
            authorization.leave_publish(id);
        }
        // A stopped holder can close its ledger during the operation. The
        // existing mapping remains in the same retained withdrawal record.
        if !permitted {
            return Err(PendFail::Dead);
        }
        result?;
        return Ok((mapping.initial_range().0.as_usize(), size));
    }
    drop(commit);
    meta.reserve_records(1)?;
    // 来源已在手上：先选址登记，再按来源范围挂载，不经任何 kernel / canonical 视图。
    let reservation = space
        .reserve(
            Placement::lowest(RegionKind::Normal, size, PAGE_SIZE)
                .map_err(|e| pend_map_error(e.into()))?,
        )
        .map_err(pend_map_error)?;
    let id = reservation.id();
    let commit = crate::basis::commit();
    if !meta.alive() || authority.modifiable().is_none() {
        drop(commit);
        let _ = space.cancel_reservation(reservation);
        return Err(PendFail::Dead);
    }
    if let (Some(authorization), Some(window)) = (&authorization, &window) {
        let (offset, count) = window.window();
        if let Err(error) = authorization.check_publish(offset, count) {
            drop(commit);
            let _ = space.cancel_reservation(reservation);
            return Err(error);
        }
    }
    if let Some(authorization) = &authorization
        && let Err(error) = space
            .reservations
            .claim_reservation(id, space, authorization.clone())
    {
        let _ = space.cancel_reservation(reservation);
        space.retire_reservation(id);
        return Err(error);
    }
    if let (Some(authorization), Some(window)) = (&authorization, &window)
        && let Err(error) = authorization.enter_publish_range(id, window.clone(), Some(&authority))
    {
        drop(commit);
        let _ = space.cancel_reservation(reservation);
        space.retire_reservation(id);
        return Err(error);
    }
    drop(commit);
    let mapping = match space.attach(reservation, source, access, access) {
        Ok(mapping) => mapping,
        Err(error) => {
            if let Some(authorization) = &authorization {
                authorization.leave_publish(id);
            }
            space.retire_reservation(id);
            return Err(pend_map_error(error));
        }
    };
    let address = mapping.initial_start().as_usize();
    let recorded = {
        let _commit = crate::basis::commit();
        let result = meta.record(token, space, mapping.clone());
        if result.is_ok() && authorization.is_some() {
            space.reservations.commit(id, mapping.clone());
        }
        result
    };
    if let Err(rejected) = recorded {
        drop(mapping);
        let _ = space.dispose(rejected);
        space.retire_reservation(id);
        if let Some(authorization) = &authorization {
            authorization.leave_publish(id);
        }
        return Err(PendFail::Dead);
    }
    if let Some(authorization) = &authorization {
        authorization.leave_publish(id);
    }
    Ok((address, size))
}

pub(crate) fn shut(meta: &Arc<Sharing>, token: PendId) -> Result<(), PendFail> {
    let _operation = SharingOperation::acquire(meta).ok_or(PendFail::Busy)?;
    meta.close_maps(Some(token))
}

/// 设备区域：借用的物理区间代替存储保留。
///
/// 既不清零、不按 RAM 回收，也不占内核 VA；实际 CPU 访问另建普通 `Span`。
pub(crate) fn region(base: usize, reg: usize, owner: TaskId) -> Result<Arc<Sharing>, PendFail> {
    let end = base
        .checked_add(reg)
        .filter(|_| reg != 0)
        .ok_or(PendFail::NotAligned)?;
    let lo = base & !(PAGE_SIZE - 1);
    let hi = end
        .checked_next_multiple_of(PAGE_SIZE)
        .ok_or(PendFail::NotAligned)?;
    let size = hi - lo;
    let region =
        unsafe { BorrowedRegion::new(PhysAddr::from_raw(lo), size) }.map_err(pend_source_error)?;
    let source = Flat::from_region(region)
        .map_err(pend_source_error)?
        .whole();
    Sharing::new(source, size, owner)
}

pub(crate) fn seal(meta: &Sharing) {
    meta.live.store(false, Ordering::Release);
    sign::wipe(WakeKey::seal(
        kernel_api::model::view::ViewKind::Share,
        meta.id,
    ));
}
