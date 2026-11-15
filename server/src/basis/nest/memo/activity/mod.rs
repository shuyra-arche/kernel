//! Memo-owned sampling/history. References retain progress, not authorization
//! or physical Pages. Replacement cannot inherit another generation's facts.
mod epoch;
use crate::{
    basis::{
        nest::{Nest, Span},
        pend::Pend,
    },
    metal::{
        mmu::{activity, completion::Pending},
        sync::LegacyLock,
    },
};
use alloc::sync::Arc;
pub(crate) use epoch::Snapshot;
use epoch::{History, Phase};
use kernel_api::{
    address::{PhysAddr, VirtAddr},
    model::pend::{NestRights, NestScope, Operations, PendRange, PendScope},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    Denied,
    Stale,
    Busy,
    Unsupported,
    OutOfMemory,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Identity {
    pub generation: u64,
    pub page: usize,
    pub address: usize,
}
pub(crate) struct State {
    identity: Identity,
    history: LegacyLock<crate::metal::sync::order::ActivityHistory, History<Pending>>,
}
fn now() -> u64 {
    crate::metal::clock::uptime()
        .as_nanos()
        .min(u64::MAX as u128) as u64
}
fn authorized(authority: &Pend, span: &Span, page: usize) -> bool {
    authority.selects(
        Operations::Nest(NestRights::INSPECT | NestRights::SAMPLE),
        PendScope::Nest(NestScope::Reservation {
            id: kernel_api::model::nest::ReservationId::from_bytes(
                &(span.identity() as u64).to_le_bytes(),
            )
            .expect("reservation identity"),
            range: PendRange::Pages {
                offset: page,
                count: 1,
            },
        }),
    )
}
impl State {
    pub(crate) fn identity(&self) -> Identity {
        self.identity
    }
    /// One post/poll. This never dereferences the original PTE, and may finish
    /// an already-owned round after revocation/replacement without new authority.
    pub(crate) fn settle(&self) -> bool {
        let _local = unsafe { crate::metal::sync::TrapGuard::save() };
        let mut h = self.history.lock();
        if let Phase::Pending { round, .. } = &mut h.phase {
            match round {
                None => match Pending::begin() {
                    Ok(p) => *round = Some(p),
                    Err(_) => h.phase = Phase::Exhausted,
                },
                Some(p) if p.poll() => h.complete(now()),
                Some(_) => {}
            }
        }
        !matches!(h.phase, Phase::Pending { .. })
    }
}
impl Nest {
    pub(crate) fn prepare_activity(
        &self,
        span: &Arc<Span>,
        page: usize,
        generation: u64,
        authority: &Arc<Pend>,
    ) -> Result<Arc<State>, Failure> {
        if !activity::available() || self.is_external() || span.grain() != 1 {
            return Err(Failure::Unsupported);
        }
        // Physical identity is checked again while publishing this owner.
        let page_id = {
            let state = self.state.lock();
            let memo = state.memos.get(span, page).ok_or(Failure::Stale)?;
            if memo.generation() != generation {
                return Err(Failure::Stale);
            }
            memo.publication().ok_or(Failure::Busy)?.cell().id().get()
        };
        let identity = Identity {
            generation,
            page: page_id,
            address: span.position(page).map_err(|_| Failure::Stale)?,
        };
        let spare = Arc::try_new(State {
            identity,
            history: LegacyLock::new(History::new()),
        })
        .map_err(|_| Failure::OutOfMemory)?;
        let _operation = span.operation().ok_or(Failure::Busy)?;
        let result = {
            let _commit = crate::basis::commit();
            if !authorized(authority, span, page) {
                return Err(Failure::Denied);
            }
            let mut state = self.state.lock();
            self.ensure_owned(span, &state)
                .map_err(|_| Failure::Stale)?;
            let physical = state
                .memos
                .get(span, page)
                .and_then(|m| m.publication())
                .ok_or(Failure::Stale)?
                .cell()
                .physical_address();
            if !state
                .root
                .cpu()
                .map_err(|_| Failure::Unsupported)?
                .activity_supported(
                    VirtAddr::new(identity.address),
                    PhysAddr::from_raw(physical),
                    self.geometry(),
                )
            {
                return Err(Failure::Unsupported);
            }
            let memo = state.memos.existing_mut(span, page).ok_or(Failure::Stale)?;
            if memo.generation() != generation
                || memo
                    .publication()
                    .is_none_or(|p| p.cell().id().get() != page_id)
            {
                return Err(Failure::Stale);
            }
            if memo.in_flight() {
                return Err(Failure::Busy);
            }
            if let Some(known) = memo.sampling.as_ref().and_then(alloc::sync::Weak::upgrade)
                && known.identity == identity
            {
                known
            } else {
                memo.sampling = Some(Arc::downgrade(&spare));
                spare.clone()
            }
        };
        drop(spare);
        Ok(result)
    }
    /// Caller holds COMMIT. Revalidate the selected publication after output
    /// preparation, without changing a PTE or waiting for another Hart.
    pub(crate) fn current_activity(
        &self,
        span: &Arc<Span>,
        page: usize,
        sampling: &Arc<State>,
    ) -> Result<(), Failure> {
        let state = self.state.lock();
        self.ensure_owned(span, &state)
            .map_err(|_| Failure::Stale)?;
        if !state.retirement.invalidation().is_clean() {
            return Err(Failure::Busy);
        }
        let memo = state.memos.get(span, page).ok_or(Failure::Stale)?;
        if memo.generation() != sampling.identity.generation
            || memo.in_flight()
            || memo
                .publication()
                .is_none_or(|p| p.cell().id().get() != sampling.identity.page)
        {
            return Err(Failure::Stale);
        }
        let physical = memo.publication().unwrap().cell().physical_address();
        if !state
            .root
            .cpu()
            .map_err(|_| Failure::Unsupported)?
            .activity_supported(
                VirtAddr::new(sampling.identity.address),
                PhysAddr::from_raw(physical),
                self.geometry(),
            )
        {
            return Err(Failure::Unsupported);
        }
        Ok(())
    }
    /// One capture and one post/poll at most, no cross-Hart waiting. The Span's
    /// operation excludes software replacement through the short handoff; no
    /// COMMIT/Nest/Span guard survives this call or a Task scheduling boundary.
    pub(crate) fn step_activity(
        &self,
        span: &Arc<Span>,
        page: usize,
        sampling: &Arc<State>,
        authority: &Arc<Pend>,
        period: u64,
    ) -> Result<Snapshot, Failure> {
        let _operation = span.operation().ok_or(Failure::Busy)?;
        {
            let _commit = crate::basis::commit();
            if !authorized(authority, span, page) {
                return Err(Failure::Denied);
            }
            let state = self.state.lock();
            self.ensure_owned(span, &state)
                .map_err(|_| Failure::Stale)?;
            if !state.retirement.invalidation().is_clean() {
                return Err(Failure::Busy);
            }
            let memo = state.memos.get(span, page).ok_or(Failure::Stale)?;
            let identity = sampling.identity;
            if memo.generation() != identity.generation
                || memo
                    .publication()
                    .is_none_or(|p| p.cell().id().get() != identity.page)
                || memo
                    .sampling
                    .as_ref()
                    .and_then(alloc::sync::Weak::upgrade)
                    .is_none_or(|s| !Arc::ptr_eq(&s, sampling))
            {
                return Err(Failure::Stale);
            }
            if memo.in_flight() {
                return Err(Failure::Busy);
            }
            let mut h = sampling.history.lock();
            let capture = match h.phase {
                Phase::Cold => true,
                Phase::Armed { synchronized, .. } => now().saturating_sub(synchronized) >= period,
                Phase::Exhausted => return Err(Failure::Unsupported),
                Phase::Pending { .. } => false,
            };
            if capture {
                let physical = memo.publication().unwrap().cell().physical_address();
                let bits = state
                    .root
                    .cpu()
                    .map_err(|_| Failure::Unsupported)?
                    .capture_activity(
                        VirtAddr::new(identity.address),
                        PhysAddr::from_raw(physical),
                        self.geometry(),
                    )
                    .map_err(|e| match e {
                        crate::basis::nest::NestError::NotAligned => Failure::Unsupported,
                        crate::basis::nest::NestError::NotMapped => Failure::Stale,
                        _ => Failure::Busy,
                    })?;
                h.capture(bits, now());
            }
        }
        sampling.settle();
        Ok(sampling.history.lock().snapshot())
    }
}
