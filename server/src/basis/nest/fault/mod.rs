//! Native Reservation fault recovery. The Reservation owns the route and queue;
//! Task continuations retain one exact event, never a new data authority.
mod binding;
mod queue;
pub(crate) mod read;
pub(crate) mod wait;
use super::{Access, Nest, Span};
use crate::basis::{pend, sign, unit::task::Task};
use alloc::sync::{Arc, Weak};
use binding::Binding;
use kernel_api::{
    abi::memory::{Failure, Observation, Query, Snapshot, Value},
    model::nest::{FaultEpoch, FaultPhase, FaultRoute, FaultStatus, NestId, ReservationId},
};
use queue::Queue;

pub(crate) struct State {
    native: bool,
    epoch: FaultEpoch,
    route: Route,
    queue: Queue,
}
enum Route {
    Disabled,
    Active(Arc<Binding>),
    Lost(Arc<Binding>),
}
impl Route {
    fn binding(&self) -> Option<&Arc<Binding>> {
        match self {
            Self::Disabled => None,
            Self::Active(binding) | Self::Lost(binding) => Some(binding),
        }
    }
}
impl State {
    pub fn new() -> Self {
        Self {
            native: false,
            epoch: FaultEpoch::INITIAL,
            route: Route::Disabled,
            queue: Queue::new(),
        }
    }
    pub(crate) fn native(&self) -> bool {
        self.native
    }
    pub(crate) fn enable_native(&mut self) {
        self.native = true;
    }
    fn binding(&self) -> Option<Arc<Binding>> {
        self.route.binding().cloned()
    }
    fn lost(&self) -> bool {
        matches!(self.route, Route::Lost(_))
    }
    fn lose(&mut self) {
        if let Route::Active(binding) = &self.route {
            self.route = Route::Lost(binding.clone());
        }
        self.queue.invalidate(None, Failure::HandlerGone);
    }
    fn configure(
        &mut self,
        epoch: FaultEpoch,
        binding: Option<Arc<Binding>>,
    ) -> Option<Arc<Binding>> {
        self.queue.invalidate(None, Failure::HandlerGone);
        self.epoch = epoch;
        let old = core::mem::replace(
            &mut self.route,
            binding.map_or(Route::Disabled, Route::Active),
        );
        match old {
            Route::Disabled => None,
            Route::Active(binding) | Route::Lost(binding) => Some(binding),
        }
    }
    pub(in crate::basis::nest) fn invalidate(&self, page: Option<usize>, error: Failure) {
        self.queue.invalidate(page, error);
    }
    pub(in crate::basis::nest) fn close(&mut self) {
        self.lose();
    }
    pub(in crate::basis::nest) fn publishing(&self, page: usize, before: u64, after: u64) {
        self.queue.publishing(
            page,
            before,
            kernel_api::model::nest::MemoGeneration::new(after).expect("publication generation"),
        );
    }
}
// Caller owns COMMIT. No route lock crosses a Nest, Span or authority lookup.
fn active(span: &Span) -> Result<Arc<Binding>, Failure> {
    let binding = span.faults.lock().binding().ok_or(Failure::NoHandler)?;
    let live = binding.live(span);
    let mut state = span.faults.lock();
    if state.lost() || !live {
        state.lose();
        return Err(Failure::HandlerGone);
    }
    Ok(binding)
}
pub(in crate::basis::nest) fn wake_span(span: &Span) {
    let binding = span.faults.lock().binding();
    if let Some(binding) = binding {
        binding.wake_later();
    }
}
pub(in crate::basis::nest) fn wake_nest(nest: &Nest) {
    let mut after = 0;
    while let Some(entry) = nest.reservations.next(after) {
        after = entry.binding.id();
        if let Some(span) = entry.reservation() {
            wake_span(span);
        }
    }
}
fn span_key(nest: &Nest, span: &Span) -> sign::WakeKey {
    sign::WakeKey::Reservation {
        space: nest.id(),
        reservation: span.identity(),
    }
}
fn full_route(
    actor: &Arc<Task>,
    token: kernel_api::model::identity::PendId,
    id: NestId,
    reservation: ReservationId,
    rights: kernel_api::model::pend::NestRights,
) -> Result<(pend::access::Reservation, Arc<Span>), Failure> {
    let route =
        pend::access::Reservation::acquire(actor, token, rights).map_err(|_| Failure::Denied)?;
    if route.nest().id() != id || route.id() != reservation {
        return Err(Failure::Denied);
    }
    let span = route.span().map_err(|_| Failure::Dead)?;
    if !binding::full(route.entry(), &span) {
        return Err(Failure::Denied);
    }
    Ok((route, span))
}
pub(crate) fn configure(
    actor: &Arc<Task>,
    action: kernel_api::abi::memory::Action,
) -> Result<Value, Failure> {
    let kernel_api::abi::memory::Action::FaultRoute {
        token,
        nest,
        reservation,
        expected,
        epoch,
        route: config,
    } = action
    else {
        return Err(Failure::Invalid);
    };
    let (route, span) = full_route(
        actor,
        token,
        nest,
        reservation,
        kernel_api::abi::memory::rights::FAULT,
    )?;
    let _operation = span.operation().ok_or(Failure::Busy)?;
    if route.nest().is_external() {
        return Err(Failure::Unsupported);
    }
    if span.version() != expected || span.faults.lock().epoch != epoch {
        return Err(Failure::Stale);
    }
    let version = expected.next().ok_or(Failure::VersionExhausted)?;
    let epoch = epoch.next().ok_or(Failure::VersionExhausted)?;
    let binding = config
        .map(|config| Binding::prepare(actor, route.entry(), &span, config))
        .transpose()?;
    let old = {
        let nest_state = route.nest().state.lock();
        let span_state = span.state.lock();
        if !nest_state.is_open() || span_state.phase != super::address::span::ReservationPhase::Open
        {
            return Err(Failure::Dead);
        }
        if span.version() != expected {
            return Err(Failure::Stale);
        }
        let mut state = span.faults.lock();
        span.changed();
        state.configure(epoch, binding)
    };
    if let Some(old) = old {
        sign::signal(old.key());
    }
    Ok(Value::FaultRoute {
        nest,
        reservation,
        version,
        epoch,
        phase: if config.is_some() {
            FaultPhase::Active
        } else {
            FaultPhase::Disabled
        },
    })
}
pub(crate) fn inspect(actor: &Arc<Task>, query: Query) -> Result<Observation, Failure> {
    let Query::FaultRoute {
        token,
        nest,
        reservation,
    } = query
    else {
        return Err(Failure::Invalid);
    };
    let _commit = crate::basis::commit();
    let (_, span) = full_route(
        actor,
        token,
        nest,
        reservation,
        kernel_api::model::pend::NestRights::INSPECT,
    )?;
    let active = active(&span);
    let state = span.faults.lock();
    let status = FaultStatus {
        nest,
        reservation,
        epoch: state.epoch,
        phase: match active {
            Ok(_) if !state.lost() => FaultPhase::Active,
            Err(Failure::NoHandler) => FaultPhase::Disabled,
            _ => FaultPhase::Lost,
        },
        handler: state.route.binding().map(|binding| binding.handler),
        timeout_ns: state
            .route
            .binding()
            .map_or(0, |binding| binding.config.timeout_ns),
        last_sequence: state.queue.last(),
    };
    Observation::new(Snapshot::FaultRoute(status)).ok_or(Failure::Invalid)
}
