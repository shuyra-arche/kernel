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
    abi::memory::{self, Delivery, Failure, Frame, Reply, Request},
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
    let call = frame.domain_snapshot(&bytes).ok_or(Failure::Invalid)?;
    let actor = super::memory::caller(&ident).ok_or(Failure::Dead)?;
    if actor.realtime() {
        return Err(Failure::Denied);
    }
    let output = match call {
        memory::DomainCall::Flat(request) => {
            let action = request.action();
            if let memory::flat::Action::Inspect(query) = action {
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
            if let memory::flat::Action::InspectResult { request: id } = action {
                let reply = actor.inspect_result(memory::Domain::Flat, id)?;
                return copy_result(&ident, &frame, reply.ok_or(Failure::Busy)?.to_bytes());
            }
            let key = memory::journal::RequestKey::new_from_domain(
                request.id(),
                memory::Opcode::Flat(action.opcode()),
                request.to_bytes(),
            )
            .ok_or(Failure::Invalid)?;
            let mut receipt = match actor.admit_memory_key(key)? {
                Admission::Replay(reply) => return copy_result(&ident, &frame, reply.to_bytes()),
                Admission::Execute(receipt) => receipt,
            };
            let result = execute_flat(&actor, action, &mut receipt);
            receipt.finish(result)
        }
        memory::DomainCall::Nest(request) => {
            let action = request.action();
            if let memory::nest::Action::Inspect(query) = action {
                if matches!(query, memory::nest::Query::FaultEvent { .. }) {
                    let wait = basis::nest::fault::read::Read::new(
                        &actor,
                        query,
                        VirtAddr::new(frame.output()),
                    )?;
                    drop(actor);
                    drop(ident);
                    return Ok(match basis::nest::fault::read::park(wait) {
                        basis::room::Handoff::Resume(()) => basis::room::Handoff::Resume(true),
                        basis::room::Handoff::Switch(pa) => basis::room::Handoff::Switch(pa),
                    });
                }
                let output = super::nest::next::inspect(&actor, query)?.to_bytes();
                return copy_result(&ident, &frame, output);
            }
            if let memory::nest::Action::InspectResult { request: id } = action {
                let reply = actor.inspect_result(memory::Domain::Nest, id)?;
                return copy_result(&ident, &frame, reply.ok_or(Failure::Busy)?.to_bytes());
            }
            let key = memory::journal::RequestKey::new_from_domain(
                request.id(),
                memory::Opcode::Nest(action.opcode()),
                request.to_bytes(),
            )
            .ok_or(Failure::Invalid)?;
            let mut receipt = match actor.admit_memory_key(key)? {
                Admission::Replay(reply) => return copy_result(&ident, &frame, reply.to_bytes()),
                Admission::Execute(receipt) => receipt,
            };
            let result = execute_nest(&actor, action, &mut receipt);
            receipt.finish(result)
        }
    };
    drop(root);
    crate::basis::nest::retirement::maintain();
    copy_result(&ident, &frame, output)
}

fn copy_result(
    ident: &Arc<TaskIdent>,
    frame: &Frame,
    output: [u8; Reply::WIDTH],
) -> Result<crate::basis::room::Handoff<bool>, Failure> {
    if !ident
        .team
        .nest
        .copy_out(&output, VirtAddr::new(frame.output()))
    {
        return Err(Failure::ReplyFault);
    }
    Ok(basis::room::Handoff::Resume(false))
}

fn execute_flat(
    actor: &Arc<Task>,
    action: memory::flat::Action,
    receipt: &mut Receipt<'_>,
) -> Result<memory::Value, Failure> {
    if matches!(
        action,
        memory::flat::Action::Block { .. }
            | memory::flat::Action::Allow { .. }
            | memory::flat::Action::Delete { .. }
    ) {
        return super::flat::next::effect(actor, action, receipt);
    }
    let _serial = actor.pend.lock();
    let prepared = super::flat::next::prepare(actor, action);
    let _commit = basis::commit();
    prepared.and_then(|prepared| super::flat::next::mutate(actor, action, prepared))
}

fn execute_nest(
    actor: &Arc<Task>,
    action: memory::nest::Action,
    receipt: &mut Receipt<'_>,
) -> Result<memory::Value, Failure> {
    if matches!(
        action,
        memory::nest::Action::Create { .. }
            | memory::nest::Action::Delete { .. }
            | memory::nest::Action::Release { .. }
            | memory::nest::Action::Publish { .. }
            | memory::nest::Action::Retract { .. }
            | memory::nest::Action::Protect { .. }
    ) {
        return super::nest::next::effect(actor, action, receipt);
    }
    let _serial = actor.pend.lock();
    let _commit = basis::commit();
    super::nest::next::mutate(actor, action)
}
