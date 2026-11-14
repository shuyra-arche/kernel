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
use crate::basis::nest::NestError;
use crate::basis::pend::Pend;
use crate::basis::pend::SourceAuthorization;
use crate::basis::sign;
use crate::basis::sign::WakeKey;
use crate::basis::unit::anchor::Anchor;
use crate::metal::sync::Level;
use crate::metal::sync::SpinLock;
use kernel_api::abi::call::pend::PendFail;
use kernel_api::address::PAGE_SIZE;
use kernel_api::address::PhysAddr;
use kernel_api::model::identity::PendId;
use kernel_api::model::identity::TaskId;

use authority::Authority;
use mapping::HeldMap;

pub(crate) use close::pending_close;
pub(crate) use close::{close, poll_close};

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
