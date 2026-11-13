//! One bounded CPU continuation. A wake hint cannot complete it; only the exact
//! accepted publication, current source authority and visible PTE can do so.
use super::queue::{Event, Progress};
use super::*;
use crate::basis::{
    room::Handoff,
    unit::task::{Resumption, WaitCall},
};
use crate::metal::{clock, generic::trap::FaultKind};
use kernel_api::{
    address::VirtAddr,
    model::{
        flat::SlotPhase,
        nest::{FaultAccess, FaultCause, FaultEvent, FaultSequence, MemoPhase},
    },
};

#[derive(Clone)]
pub(crate) struct Cpu {
    nest: Arc<Nest>,
    span: Arc<Span>,
    binding: Arc<Binding>,
    event: Arc<Event>,
    address: VirtAddr,
    kind: FaultKind,
    deadline: u64,
}
pub(super) fn access(kind: FaultKind) -> (Access, FaultAccess) {
    match kind {
        FaultKind::Load => (Access::READ, FaultAccess::Read),
        FaultKind::Store => (Access::WRITE, FaultAccess::Write),
        FaultKind::Instruction => (Access::EXECUTE, FaultAccess::Execute),
    }
}
impl Cpu {
    /// Caller holds COMMIT through fault acceptance. No allocation supplies data.
    pub(crate) fn admit(
        actor: &Arc<Task>,
        nest: Arc<Nest>,
        span: Arc<Span>,
        address: VirtAddr,
        kind: FaultKind,
    ) -> Result<Option<Self>, Failure> {
        let _commit = crate::basis::commit();
        if actor.realtime() || nest.is_external() {
            return Err(Failure::Unsupported);
        }
        if matches!(
            nest.probe_fault(address, kind),
            super::super::FaultOutcome::Resolved
        ) {
            return Ok(None);
        }
        let (need, access) = access(kind);
        let page = span.page_index(address) / span.grain() * span.grain();
        if !span
            .access(page)
            .map_err(|_| Failure::NoRegion)?
            .1
            .contains(need)
        {
            return Err(Failure::Denied);
        }
        let memo = nest
            .inspect_logical(&span, page)
            .map_err(|_| Failure::Denied)?;
        let source = nest
            .logical_source(&span, page, memo.generation.get())
            .map_err(|_| Failure::Stale)?
            .ok_or(Failure::SourceEmpty)?;
        let range = source.check(need).map_err(|_| Failure::Denied)?;
        source
            .source()
            .check_publish(source.window().0, source.window().1)
            .map_err(|_| Failure::Busy)?;
        let slot = range.inspect_slot().map_err(|_| Failure::Unsupported)?;
        let cause = match slot.phase {
            SlotPhase::Fresh => FaultCause::Fresh,
            SlotPhase::Missing => FaultCause::Missing,
            _ => return Err(Failure::Busy),
        };
        let now = clock::uptime().as_nanos() as u64;
        let binding = active(&span)?;
        let (binding, event) = {
            let mut state = span.faults.lock();
            if state.lost {
                return Err(Failure::HandlerGone);
            }
            if binding.handler == actor.ident.id {
                return Err(Failure::Unsupported);
            }
            let deadline = binding
                .config
                .deadline(now)
                .ok_or(Failure::VersionExhausted)?;
            let wire = FaultEvent {
                nest: nest.id(),
                reservation: memo.reservation,
                epoch: state.epoch,
                sequence: FaultSequence::new(1).unwrap(),
                memo_page: page,
                generation: memo.generation,
                flat: slot.flat,
                slot_page: slot.coordinate,
                version: slot.version,
                cause,
                access,
            };
            let event = state.queue.accept(wire, deadline, now)?;
            if binding.notify().is_err() {
                state.lost = true;
                state.queue.invalidate(None, Failure::HandlerGone);
                binding.wake_later();
                return Err(Failure::HandlerGone);
            }
            (binding, event)
        };
        let deadline = clock::deadline(kernel_api::model::wait::Wait::At(event.deadline))
            .expect("finite fault deadline");
        // Queue admission precedes both the service hint and waiter registration.
        sign::signal(binding.key());
        Ok(Some(Self {
            nest,
            span,
            binding,
            event,
            address,
            kind,
            deadline,
        }))
    }
    pub fn deadline(&self) -> u64 {
        self.deadline
    }
    fn poll(&self) -> Result<bool, Failure> {
        {
            let state = self.span.faults.lock();
            if state.epoch != self.event.wire.epoch {
                return Err(Failure::HandlerGone);
            }
        }
        active(&self.span)?;
        if clock::now().as_ticks() >= self.deadline {
            self.event.finish(Err(Failure::TimedOut));
            return Err(Failure::TimedOut);
        }
        let publication = match self.event.progress() {
            Progress::Terminal(Err(error)) => return Err(error),
            Progress::Terminal(Ok(())) => self.event.wire.generation.next(),
            Progress::Pending { publication } => publication,
        };
        let wire = self.event.wire;
        let memo = self
            .nest
            .inspect_logical(&self.span, wire.memo_page)
            .map_err(|_| Failure::Stale)?;
        if memo.generation != wire.generation && Some(memo.generation) != publication {
            return Err(Failure::Stale);
        }
        let source = self
            .nest
            .logical_source(&self.span, wire.memo_page, memo.generation.get())
            .map_err(|_| Failure::Stale)?
            .ok_or(Failure::Stale)?;
        let range = source
            .check(access(self.kind).0)
            .map_err(|_| Failure::Denied)?;
        if range.source_id() != wire.flat || range.index().get() != wire.slot_page {
            return Err(Failure::Stale);
        }
        if publication != Some(memo.generation) || memo.phase != MemoPhase::Published {
            return Ok(false);
        }
        let principal = self
            .nest
            .state
            .lock()
            .memos
            .get(&self.span, wire.memo_page)
            .and_then(|memo| memo.principal())
            .ok_or(Failure::Denied)?;
        principal
            .check(&self.nest, &self.span, wire.memo_page, access(self.kind).0)
            .map_err(|_| Failure::Denied)?;
        source
            .source()
            .check_publish(source.window().0, source.window().1)
            .map_err(|_| Failure::Busy)?;
        source.admit_provenance(None).map_err(|_| Failure::Denied)?;
        // The page and current logical selection must still be the published one.
        let page = self
            .nest
            .memo_page(&self.span, wire.memo_page)
            .map_err(|_| Failure::Stale)?
            .ok_or(Failure::Stale)?;
        source.same_page(&page).map_err(|_| Failure::Stale)?;
        if !matches!(
            self.nest.probe_fault(self.address, self.kind),
            super::super::FaultOutcome::Resolved
        ) {
            return Ok(false);
        }
        self.event.finish(Ok(()));
        Ok(true)
    }
}
pub(crate) fn park(wait: Cpu) -> Result<Handoff<()>, Failure> {
    loop {
        let commit = crate::basis::commit();
        if wait.poll()? {
            return Ok(Handoff::Resume(()));
        }
        let blocked = sign::block_with_call::<kernel_api::abi::call::room::RoomFail>(
            wait.binding.key(),
            wait.binding.life(),
            clock::remaining(Some(wait.deadline)),
            Some(WaitCall::Fault(wait.clone())),
        )
        .map_err(|_| Failure::OutOfMemory)?;
        drop(commit);
        match sign::finish_block(blocked) {
            Handoff::Resume(()) => {}
            other => return Ok(other),
        }
    }
}
pub(crate) fn resume(task: &Arc<Task>, wait: Cpu) -> Resumption {
    let _commit = crate::basis::commit();
    let result = wait.poll();
    if matches!(result, Ok(false)) {
        if sign::requeue(
            task,
            wait.binding.key(),
            wait.binding.life(),
            clock::remaining(Some(wait.deadline)),
            WaitCall::Fault(wait.clone()),
            (),
        )
        .is_ok()
        {
            return Resumption::Waiting;
        }
    }
    if let Err(error) = result {
        wait.event.finish(Err(error));
        crate::basis::unit::lifecycle::fault(task, error as i64 as usize);
        return Resumption::Terminated;
    } else if matches!(result, Ok(false)) {
        crate::basis::unit::lifecycle::fault(task, Failure::OutOfMemory as i64 as usize);
        return Resumption::Terminated;
    }
    Resumption::Complete
}
