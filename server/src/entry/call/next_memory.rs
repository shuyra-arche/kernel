//! Native Flat/Nest memory provider.
//! Mutations use the actual Task's bounded result journal; finite event reads
//! retain a scheduler continuation and revalidate the exact live route on delivery.
use crate::{
    basis::{
        self,
        unit::task::{Task, TaskIdent},
    },
    metal::switcher::context::{Gprs, TrapContext},
};
use alloc::sync::Arc;
use basis::unit::task::request::Admission;
pub(super) use basis::unit::task::request::Receipt;
use kernel_api::{
    abi::memory::{self, Action, Delivery, Failure, Frame, Reply, Request},
    address::VirtAddr,
};

pub(super) fn dispatch(
    frame: &mut TrapContext,
    ident: Arc<TaskIdent>,
    number: u64,
    registers: [usize; 6],
    root: crate::basis::sync::Root<'_>,
) -> *mut TrapContext {
    let delivery = match run(ident, number, registers, root) {
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
    mut root: crate::basis::sync::Root<'_>,
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
    if let Action::Inspect(query) = request.action()
        && query.domain() == memory::Domain::Flat
    {
        let output = super::flat::next::inspect(&mut root, &actor, query)?.to_bytes();
        if !ident
            .team
            .nest
            .copy_out(&output, VirtAddr::new(frame.output()))
        {
            return Err(Failure::ReplyFault);
        }
        return Ok(basis::room::Handoff::Resume(false));
    }
    drop(root);
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
            memory::Domain::Flat => return Err(Failure::Unsupported),
            memory::Domain::Nest => super::nest::next::inspect(actor, query),
        }
        .map(|value| value.to_bytes());
    }
    let mut receipt = match actor.admit_memory(request)? {
        Admission::Replay(reply) => return Ok(reply.to_bytes()),
        Admission::Execute(receipt) => receipt,
    };
    if matches!(
        request.action(),
        Action::Block { .. } | Action::Allow { .. } | Action::DeleteFlat { .. }
    ) {
        let result = super::flat::next::effect(actor, request.action(), &mut receipt);
        return Ok(receipt.finish(result));
    }
    if matches!(
        request.action(),
        Action::CreateNest { .. }
            | Action::Publish { .. }
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
