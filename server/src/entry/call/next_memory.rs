//! Native memory provider; dispatch requires the coherent memory-v11 image feature.
//! Mutations use the actual Task's bounded result journal; finite event reads
//! retain a scheduler continuation and revalidate the exact live route on delivery.
#[path = "next_memory/receipt.rs"]
mod receipt;
use crate::{
    basis::{
        self,
        unit::task::{Task, TaskIdent},
    },
    metal::switcher::context::{Gprs, TrapContext},
};
use alloc::{boxed::Box, sync::Arc};
use kernel_api::{
    abi::memory::{
        self, Action, Delivery, Failure, Frame, Outcome, Reply, Request,
        journal::{Admission, Journal},
    },
    address::VirtAddr,
};
pub(super) use receipt::Receipt;

pub(super) fn dispatch(
    frame: &mut TrapContext,
    ident: Arc<TaskIdent>,
    number: u64,
    registers: [usize; 6],
) -> *mut TrapContext {
    let delivery = match run(ident, number, registers) {
        Ok(crate::basis::room::Handoff::Switch(pa)) => return pa as *mut TrapContext,
        Ok(crate::basis::room::Handoff::Resume(true)) => return frame as *mut TrapContext,
        Ok(crate::basis::room::Handoff::Resume(false)) => Delivery::Written,
        Err(error) => Delivery::Failed(error),
    };
    for (register, word) in [Gprs::A0, Gprs::A1, Gprs::A2]
        .into_iter()
        .zip(delivery.registers())
    {
        frame.gpr.set_x(register, word as usize);
    }
    frame as *mut TrapContext
}

fn run(
    ident: Arc<TaskIdent>,
    number: u64,
    registers: [usize; 6],
) -> Result<crate::basis::room::Handoff<bool>, Failure> {
    let frame = Frame::decode(number, registers).ok_or(Failure::Invalid)?;
    let mut bytes = [0; Request::WIDTH];
    if !ident
        .team
        .nest
        .copy_in(&mut bytes, VirtAddr::new(frame.input()))
    {
        return Err(Failure::Invalid);
    }
    let request = frame.snapshot(&bytes).ok_or(Failure::Invalid)?.request();
    let actor = super::memory::caller(&ident).ok_or(Failure::Dead)?;
    if actor.realtime() {
        return Err(Failure::Denied);
    }
    if let Action::Inspect(query @ memory::Query::FaultEvent { .. }) = request.action() {
        let wait =
            basis::nest::fault::read::Read::new(&actor, query, VirtAddr::new(frame.output()))?;
        drop(actor);
        drop(ident);
        return Ok(match basis::nest::fault::read::park(wait) {
            basis::room::Handoff::Resume(()) => basis::room::Handoff::Resume(true),
            basis::room::Handoff::Switch(pa) => basis::room::Handoff::Switch(pa),
        });
    }
    let output = execute(&actor, request)?;
    // A completed holding change reopens its Page gate. Wake source observers
    // only after the ledger, journal and COMMIT guards have been released.
    crate::basis::cell::change::maintain();
    if !ident
        .team
        .nest
        .copy_out(&output, VirtAddr::new(frame.output()))
    {
        return Err(Failure::ReplyFault);
    }
    Ok(basis::room::Handoff::Resume(false))
}

fn execute(actor: &Arc<Task>, request: Request) -> Result<[u8; Reply::WIDTH], Failure> {
    if let Action::Inspect(query) = request.action() {
        return match query.domain() {
            memory::Domain::Flat => super::flat::next::inspect(actor, query),
            memory::Domain::Nest => super::nest::next::inspect(actor, query),
        }
        .map(|value| value.to_bytes());
    }
    let _serial = actor.pend.lock();
    let admission_commit = basis::commit();
    let mut window = actor.memory_results.lock();
    if window.is_none() {
        let journal = Journal::new(actor.ident.id).ok_or(Failure::Denied)?;
        *window = Some(Box::try_new(journal).map_err(|_| Failure::OutOfMemory)?);
    }
    let journal = window.as_mut().expect("prepared Task journal");
    if let Action::InspectResult {
        domain,
        request: id,
    } = request.action()
    {
        return match journal.inspect(actor.ident.id, domain, id) {
            Ok(Some(reply)) => Ok(reply.to_bytes()),
            Ok(None) => Err(Failure::Busy),
            Err(Failure::ResultExpired) => {
                Ok(
                    Reply::new(id, memory::Opcode::Inspect(domain), Outcome::Expired)
                        .expect("expired result")
                        .to_bytes(),
                )
            }
            Err(error) => Err(error),
        };
    }
    let accepted = match journal.begin(actor.ident.id, request)? {
        Admission::Replay(reply) | Admission::InFlight(Some(reply)) => return Ok(reply.to_bytes()),
        Admission::InFlight(None) => return Err(Failure::Busy),
        Admission::Execute(accepted) => accepted,
    };
    drop(window);
    drop(admission_commit);
    drop(_serial);
    let mut receipt = Receipt::new(actor, request, accepted);
    if matches!(
        request.action(),
        Action::Block { .. } | Action::Allow { .. } | Action::DeleteFlat { .. }
    ) {
        let result = super::flat::next::effect(actor, request.action(), &mut receipt);
        return Ok(receipt.finish(result));
    }
    if matches!(
        request.action(),
        Action::Publish { .. }
            | Action::Retract { .. }
            | Action::Protect { .. }
            | Action::Release { .. }
            | Action::DeleteNest { .. }
    ) {
        let result = super::nest::next::effect(actor, request.action(), &mut receipt);
        return Ok(receipt.finish(result));
    }
    let _serial = actor.pend.lock();
    // Occupy allocates/zeroes before COMMIT, then rechecks both live selection
    // and expected version. A losing preparation remains private and is freed.
    let prepared = super::flat::next::prepare(actor, request.action());
    let _commit = basis::commit();
    let result = prepared.and_then(|prepared| match request.action().opcode().domain() {
        memory::Domain::Flat => super::flat::next::mutate(actor, request.action(), prepared),
        memory::Domain::Nest => super::nest::next::mutate(actor, request.action()),
    });
    Ok(receipt.finish(result))
}
