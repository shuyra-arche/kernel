//! Staged next-version provider. Dispatch remains disabled by the active ABI.
//! Implemented residency commits use the actual Task's bounded result journal;
//! unsupported lifecycle/maintenance operations fail before resource effects.
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

pub(super) fn dispatch(
    frame: &mut TrapContext,
    ident: &Arc<TaskIdent>,
    number: u64,
    registers: [usize; 6],
) {
    let delivery =
        run(ident, number, registers).map_or_else(Delivery::Failed, |()| Delivery::Written);
    for (register, word) in [Gprs::A0, Gprs::A1, Gprs::A2]
        .into_iter()
        .zip(delivery.registers())
    {
        frame.gpr.set_x(register, word as usize);
    }
}

fn run(ident: &Arc<TaskIdent>, number: u64, registers: [usize; 6]) -> Result<(), Failure> {
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
    let actor = super::memory::caller(ident).ok_or(Failure::Dead)?;
    if actor.realtime() {
        return Err(Failure::Denied);
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
    Ok(())
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
    // Occupy allocates/zeroes before COMMIT, then rechecks both live selection
    // and expected version. A losing preparation remains private and is freed.
    let prepared = super::flat::next::prepare(actor, request.action());
    let _commit = basis::commit();
    let result = prepared.and_then(|prepared| match request.action().opcode().domain() {
        memory::Domain::Flat => super::flat::next::mutate(actor, request.action(), prepared),
        memory::Domain::Nest => super::nest::next::mutate(actor, request.action()),
    });
    let outcome = match result {
        Ok(value) => Outcome::Committed(value),
        Err(error) => Outcome::Rejected(error),
    };
    let reply = Reply::new(
        request.id().expect("mutation identity"),
        request.action().opcode(),
        outcome,
    )
    .expect("operation result");
    journal
        .record(&accepted, reply)
        .expect("record before output while resource commit remains held");
    Ok(reply.to_bytes())
}
