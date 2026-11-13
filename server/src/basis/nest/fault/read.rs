//! Non-consuming event reads, including a finite scheduler continuation. The
//! caller's exact branch, route epoch and output address are checked on delivery.
use super::*;
use crate::basis::{
    room::Handoff,
    unit::task::{Resumption, WaitCall},
};
use crate::metal::{
    clock,
    switcher::context::{Gprs, TrapContext},
};
use kernel_api::{
    abi::memory::Delivery,
    address::VirtAddr,
    model::{nest::FaultEvent, wait::Wait},
};

#[derive(Clone)]
pub(crate) struct Read {
    actor: Weak<Task>,
    span: Arc<Span>,
    binding: Arc<Binding>,
    query: Query,
    output: VirtAddr,
    deadline: u64,
}
// The Query's PendId is an immutable lookup key for the retained actual actor;
// it is never transferred as authority. Delivery rechecks it under COMMIT and
// copies through that actor's Nest, without retaining a Rust user-memory reference.
unsafe impl Send for Read {}
unsafe impl Sync for Read {}
impl Read {
    pub(crate) fn new(actor: &Arc<Task>, query: Query, output: VirtAddr) -> Result<Self, Failure> {
        let Query::FaultEvent {
            token,
            nest,
            reservation,
            epoch,
            wait,
            ..
        } = query
        else {
            return Err(Failure::Invalid);
        };
        let _commit = crate::basis::commit();
        let now = clock::uptime().as_nanos() as u64;
        let ns = kernel_api::model::nest::fault_read_deadline(wait, now).ok_or(Failure::Invalid)?;
        let (_, span) = full_route(
            actor,
            token,
            nest,
            reservation,
            kernel_api::model::pend::NestRights::INSPECT | kernel_api::abi::memory::rights::FAULT,
        )?;
        let binding = {
            if span.faults.lock().epoch != epoch {
                return Err(Failure::Stale);
            }
            let binding = active(&span)?;
            if !binding.reads(actor, token) {
                return Err(Failure::Denied);
            }
            binding
        };
        Ok(Self {
            actor: Arc::downgrade(actor),
            span,
            binding,
            query,
            output,
            deadline: clock::deadline(Wait::At(ns)).expect("finite read deadline"),
        })
    }
    pub fn deadline(&self) -> u64 {
        self.deadline
    }
    fn poll(&self) -> Result<FaultEvent, Failure> {
        let Query::FaultEvent {
            token,
            epoch,
            after,
            wait,
            ..
        } = self.query
        else {
            unreachable!()
        };
        let actor = self
            .actor
            .upgrade()
            .filter(|actor| pend::live(actor))
            .ok_or(Failure::Dead)?;
        if self.span.faults.lock().epoch != epoch {
            return Err(Failure::Stale);
        }
        let binding = active(&self.span)?;
        if !Arc::ptr_eq(&binding, &self.binding) || !binding.reads(&actor, token) {
            return Err(Failure::Denied);
        }
        let state = self.span.faults.lock();
        if state.lost() {
            return Err(Failure::HandlerGone);
        }
        state.queue.expire(clock::uptime().as_nanos() as u64);
        let result = state.queue.read(epoch, after);
        if result == Err(Failure::Busy)
            && wait != Wait::Poll
            && clock::now().as_ticks() >= self.deadline
        {
            return Err(Failure::TimedOut);
        }
        result
    }
    fn pending(&self, result: &Result<FaultEvent, Failure>) -> bool {
        matches!(result, Err(Failure::Busy))
            && matches!(
                self.query,
                Query::FaultEvent {
                    wait: Wait::At(_),
                    ..
                }
            )
            && clock::now().as_ticks() < self.deadline
    }
    fn write(&self, result: Result<FaultEvent, Failure>) {
        let Some(actor) = self.actor.upgrade() else {
            return;
        };
        let delivery = match result {
            Ok(event) => {
                let bytes = Observation::new(Snapshot::FaultEvent(event))
                    .expect("admitted event")
                    .to_bytes();
                if actor.ident.team.nest.copy_out(&bytes, self.output) {
                    Delivery::Written
                } else {
                    Delivery::Failed(Failure::ReplyFault)
                }
            }
            Err(error) => Delivery::Failed(error),
        };
        let frame = unsafe { &mut *(actor.ident.frame_pa.as_usize() as *mut TrapContext) };
        for (reg, word) in [Gprs::A0, Gprs::A1, Gprs::A2]
            .into_iter()
            .zip(delivery.registers())
        {
            frame.gpr.set_x(reg, word as usize);
        }
    }
}
pub(crate) fn park(wait: Read) -> Handoff<()> {
    loop {
        let commit = crate::basis::commit();
        let result = wait.poll();
        if !wait.pending(&result) {
            wait.write(result);
            return Handoff::Resume(());
        }
        let blocked = sign::block_with_call::<kernel_api::abi::call::room::RoomFail>(
            wait.binding.key(),
            wait.binding.life(),
            clock::remaining(Some(wait.deadline)),
            Some(WaitCall::FaultRead(wait.clone())),
        );
        let blocked = match blocked {
            Ok(blocked) => blocked,
            Err(_) => {
                wait.write(Err(Failure::OutOfMemory));
                return Handoff::Resume(());
            }
        };
        drop(commit);
        match sign::finish_block(blocked) {
            Handoff::Resume(()) => {}
            other => return other,
        }
    }
}
pub(crate) fn resume(task: &Arc<Task>, wait: Read) -> Resumption {
    let _commit = crate::basis::commit();
    let result = wait.poll();
    if wait.pending(&result) {
        if sign::requeue(
            task,
            wait.binding.key(),
            wait.binding.life(),
            clock::remaining(Some(wait.deadline)),
            WaitCall::FaultRead(wait.clone()),
            (),
        )
        .is_ok()
        {
            return Resumption::Waiting;
        }
        wait.write(Err(Failure::OutOfMemory));
    } else {
        wait.write(result);
    }
    Resumption::Complete
}
