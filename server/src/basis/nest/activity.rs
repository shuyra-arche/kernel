//! Bounded recipient subscriptions. Pend remains the authorization owner;
//! Memo remains the PTE/history owner; this registry owns delivery cursors only.
use crate::{
    basis::{
        cell::{self, page::Owner},
        flat::{
            Flat,
            sharing::{self, Sharing},
        },
        nest::{
            Access, Nest, Span,
            memo::activity::{Failure, Identity, Snapshot, State},
        },
        pend::{self, PendEntry, SourceAuthorization, Target},
        sign::endpoint::{self, Endpoint},
        unit::task::Task,
        view::View,
    },
    metal::sync::SpinLock,
};
use alloc::{
    sync::{Arc, Weak},
    vec::Vec,
};
use core::sync::atomic::{AtomicBool, Ordering};
use kernel_api::{
    abi::call::pend::PendFail,
    address::{PageCount, PageIndex},
    model::{
        identity::Mark,
        memory::activity::{self as protocol, Evidence, Request, Sample, SamplePage, Validity},
        modifiable::Modifiable,
        pend::{Distributable, NestRights, Operations, ShareRights},
        sign::SignRights,
    },
};
enum Slot {
    Free,
    Reserved,
    Live(Arc<Subscription>),
}
struct Registry {
    slots: [Slot; protocol::MAX_SUBSCRIPTIONS],
    cursor: usize,
}
static DELIVERY: crate::basis::name::Serial =
    crate::basis::name::Serial::new(1, isize::MAX as usize);
static REGISTRY: SpinLock<Registry> = SpinLock::new(Registry {
    slots: [const { Slot::Free }; protocol::MAX_SUBSCRIPTIONS],
    cursor: 0,
});
struct Reservation(usize);
impl Reservation {
    fn take() -> Result<Self, Failure> {
        let mut r = REGISTRY.lock();
        let at = r
            .slots
            .iter()
            .position(|s| matches!(s, Slot::Free))
            .ok_or(Failure::Busy)?;
        r.slots[at] = Slot::Reserved;
        Ok(Self(at))
    }
    fn publish(self, s: Arc<Subscription>) {
        REGISTRY.lock().slots[self.0] = Slot::Live(s);
        core::mem::forget(self);
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        REGISTRY.lock().slots[self.0] = Slot::Free;
    }
}
struct Cursor {
    epoch: u64,
    end: u64,
}
struct Delivery {
    cursor: Option<Cursor>,
    pending: Option<(u64, Cursor)>,
    stopped: Option<Validity>,
}
struct Subscription {
    actor: Weak<Task>,
    source: Arc<PendEntry>,
    nest: Arc<Nest>,
    span: Arc<Span>,
    page: usize,
    period: u64,
    identity: Identity,
    sampling: Arc<State>,
    view: Arc<Sharing>,
    output: Arc<PendEntry>,
    event: Arc<PendEntry>,
    sign: Arc<Endpoint>,
    address: usize,
    registered: u64,
    cancelled: AtomicBool,
    delivery: SpinLock<Delivery>,
}
fn now() -> u64 {
    crate::metal::clock::uptime()
        .as_nanos()
        .min(u64::MAX as u128) as u64
}
fn failure(e: PendFail) -> Failure {
    match e {
        PendFail::Busy => Failure::Busy,
        PendFail::OutOfMemory => Failure::OutOfMemory,
        _ => Failure::Denied,
    }
}
pub(crate) fn register(
    actor: &Arc<Task>,
    request: Request,
) -> Result<
    (
        kernel_api::model::identity::PendId,
        kernel_api::model::identity::PendId,
    ),
    Failure,
> {
    let slot = Reservation::take()?;
    let _serial = actor.pend.lock();
    let route = pend::access::Reservation::acquire(
        actor,
        request.position.token(),
        NestRights::INSPECT | NestRights::SAMPLE,
    )
    .map_err(failure)?;
    let page = route.position(request.position).map_err(failure)?;
    let nest = route.nest();
    let span = route.span().map_err(failure)?;
    let source = route.entry().clone();
    let sampling =
        nest.prepare_activity(&span, page, request.generation.get(), source.authority())?;
    let identity = sampling.identity();
    let cell = cell::allocate(Owner::Kernel).map_err(|_| Failure::OutOfMemory)?;
    let address = cell.physical_address();
    // Page::allocate zeros RAM. Every bit pattern of AtomicU64 is valid; this
    // uniquely owned page is typed before any mapping/publication can observe it.
    unsafe {
        core::ptr::write_bytes(address as *mut SamplePage, 0, 1);
    }
    let mut cells = Vec::new();
    cells
        .try_reserve_exact(1)
        .map_err(|_| Failure::OutOfMemory)?;
    cells.push((PageIndex::ZERO, cell));
    let flat = Flat::from_cells(Owner::Kernel, PageCount::new(1).unwrap(), cells)
        .map_err(|_| Failure::OutOfMemory)?;
    let source_root =
        SourceAuthorization::new(flat.whole(), Access::READ, actor.ident.id).map_err(failure)?;
    let view = Sharing::from_source(flat.whole(), 4096, actor.ident.id).map_err(failure)?;
    let output = Arc::try_new(
        PendEntry::try_new(View::Share(view.clone()), Mark::NONE, Modifiable::No, None)
            .map_err(failure)?,
    )
    .map_err(|_| Failure::OutOfMemory)?;
    let mut limits = output.authority().limits();
    limits.operations = Operations::Share(ShareRights::ASSOCIATE);
    limits.distributable = Distributable::No;
    output.narrow(limits).map_err(failure)?;
    let sign = Endpoint::try_new(actor.ident.id).map_err(|_| Failure::OutOfMemory)?;
    let event = Arc::try_new(
        PendEntry::try_new(Target::Sign(sign.clone()), Mark::NONE, Modifiable::No, None)
            .map_err(failure)?,
    )
    .map_err(|_| Failure::OutOfMemory)?;
    let mut limits = event.authority().limits();
    limits.operations = Operations::Sign(SignRights::LISTEN | SignRights::CONSUME);
    limits.distributable = Distributable::No;
    event.narrow(limits).map_err(failure)?;
    let s = Arc::try_new(Subscription {
        actor: Arc::downgrade(actor),
        source,
        nest: nest.clone(),
        span,
        page,
        period: request.period_ns,
        identity,
        sampling,
        view,
        output: output.clone(),
        event: event.clone(),
        sign,
        address,
        registered: now(),
        cancelled: AtomicBool::new(false),
        delivery: SpinLock::new(Delivery {
            cursor: None,
            pending: None,
            stopped: None,
        }),
    })
    .map_err(|_| Failure::OutOfMemory)?;
    let data = actor.pend.prepare(output.clone()).map_err(failure)?;
    let wake = actor.pend.prepare(event.clone()).map_err(failure)?;
    let _commit = crate::basis::commit();
    if !s.live() {
        return Err(Failure::Denied);
    }
    let claim = source_root.claim().map_err(failure)?;
    s.view.attach_source(&source_root);
    source_root
        .begin_withdraw()
        .map_err(failure)?
        .adopt(&s.view, output.token(), output.authority().clone())
        .map_err(failure)?;
    claim.commit();
    data.publish();
    wake.publish();
    let result = (output.token(), event.token());
    slot.publish(s);
    Ok(result)
}
impl Subscription {
    fn live(&self) -> bool {
        !self.cancelled.load(Ordering::Acquire)
            && self.actor.upgrade().is_some_and(|t| pend::live(&t))
            && self
                .source
                .permits(Operations::Nest(NestRights::INSPECT | NestRights::SAMPLE))
            && self.source.authority().window().is_some_and(|(o, c)| {
                self.page >= o.get()
                    && self
                        .page
                        .checked_add(1)
                        .is_some_and(|end| end <= o.get() + c.get())
            })
            && self.output.alive()
            && self.event.alive()
    }
    fn cancel(&self) {
        if !self.cancelled.swap(true, Ordering::AcqRel) {
            self.output.invalidate();
            self.event.invalidate();
            sharing::seal(&self.view);
        }
    }
    fn step(&self) -> bool {
        {
            let _commit = crate::basis::commit();
            if !self.live() {
                self.cancel();
            }
        }
        if self.cancelled.load(Ordering::Acquire) {
            return sharing::poll_close(&self.view).is_ok() && self.sampling.settle();
        }
        let sampled = self.nest.step_activity(
            &self.span,
            self.page,
            &self.sampling,
            self.source.authority(),
            self.period,
        );
        let _commit = crate::basis::commit();
        if !self.live() {
            self.cancel();
            return false;
        }
        let mut d = self.delivery.lock();
        if let Some((sequence, _)) = &d.pending {
            if self.sign.consumed() != *sequence {
                return false;
            }
            d.cursor = Some(d.pending.take().unwrap().1);
        }
        if d.stopped.is_some() {
            return false;
        }
        let snapshot = match sampled {
            Ok(s) => s,
            Err(Failure::Busy) => return false,
            Err(Failure::Denied) => {
                drop(d);
                self.cancel();
                return false;
            }
            Err(e) => {
                d.stopped = Some(if e == Failure::Unsupported {
                    Validity::Unsupported
                } else {
                    Validity::Stale
                });
                Snapshot {
                    epoch: d.cursor.as_ref().map_or(0, |c| c.epoch),
                    accessed: 0,
                    dirty: 0,
                    start: d.cursor.as_ref().map_or(0, |c| c.end),
                    end: d.cursor.as_ref().map_or(0, |c| c.end),
                    synchronized: d.cursor.as_ref().map_or(0, |c| c.end),
                }
            }
        };
        if d.cursor.is_none() {
            if snapshot.end >= self.registered && snapshot.epoch > 0 {
                d.cursor = Some(Cursor {
                    epoch: snapshot.epoch,
                    end: snapshot.end,
                });
            }
            if d.stopped.is_none() {
                return false;
            }
        }
        let cursor = d.cursor.as_ref();
        if d.stopped.is_none() && cursor.is_some_and(|c| snapshot.epoch <= c.epoch) {
            return false;
        }
        if d.stopped.is_none()
            && cursor.is_some_and(|c| snapshot.end.saturating_sub(c.end) < self.period)
        {
            return false;
        }
        let Some(sequence) = DELIVERY.allocate().map(|n| n as u64) else {
            drop(d);
            self.cancel();
            return false;
        };
        let validity = d.stopped.unwrap_or(Validity::Observed);
        let observed = |epoch: u64| {
            if validity != Validity::Observed {
                Evidence::Unknown
            } else if cursor.is_some_and(|c| epoch > c.epoch) {
                Evidence::Observed
            } else {
                Evidence::NotObserved
            }
        };
        let sample = Sample {
            sequence,
            epoch: snapshot.epoch,
            nest: self.nest.id().get() as u64,
            reservation: self.span.identity() as u64,
            position: self.page as u64,
            generation: self.identity.generation,
            page: self.identity.page as u64,
            address: self.identity.address as u64,
            bytes: 4096,
            start_ns: cursor.map_or(0, |c| c.end),
            end_ns: snapshot.end,
            synchronized_ns: snapshot.synchronized,
            validity,
            accessed: observed(snapshot.accessed),
            dirty: observed(snapshot.dirty),
        };
        // All output writes and Sign publication revalidate live authority in the
        // same COMMIT as revocation. No unread sample is replaced before Consume.
        let page = unsafe { &*(self.address as *const SamplePage) };
        if !page.publish(sample) {
            drop(d);
            self.cancel();
            return false;
        }
        match endpoint::publish(&self.sign) {
            Ok(receipt) => {
                let _ = endpoint::announce(&self.sign);
                d.pending = Some((
                    receipt,
                    Cursor {
                        epoch: snapshot.epoch,
                        end: snapshot.end,
                    },
                ));
            }
            Err(_) => {
                drop(d);
                self.cancel();
            }
        }
        false
    }
}
pub(crate) fn advance() {
    for _ in 0..8 {
        let next = {
            let mut r = REGISTRY.lock();
            let at = r.cursor;
            r.cursor = (at + 1) % r.slots.len();
            let value = match &r.slots[at] {
                Slot::Live(s) => Some(s.clone()),
                _ => None,
            };
            value.map(|s| (at, s))
        };
        if let Some((at, s)) = next {
            if s.step() {
                let old = core::mem::replace(&mut REGISTRY.lock().slots[at], Slot::Free);
                drop(old);
            }
        }
    }
}
/// Bounded lifecycle hook; callers already serialize authority changes under
/// COMMIT. Physical withdrawal is advanced outside that lock by the Kernel Task.
pub(crate) fn withdraw() {
    for at in 0..protocol::MAX_SUBSCRIPTIONS {
        let s = {
            match &REGISTRY.lock().slots[at] {
                Slot::Live(s) => Some(s.clone()),
                _ => None,
            }
        };
        if let Some(s) = s {
            if !s.live() {
                s.cancel();
            }
        }
    }
}

pub(crate) fn active() -> bool {
    REGISTRY
        .lock()
        .slots
        .iter()
        .any(|s| matches!(s, Slot::Live(_)))
}
