use crate::basis::room::Handoff;
use crate::basis::sign::block_with_call;
use crate::basis::sign::finish_block;
use crate::basis::sign::requeue;
use crate::basis::unit::task::Resumption;
use crate::basis::unit::task::Task;
use crate::basis::unit::task::WaitCall;
use alloc::sync::Arc;
use core::time::Duration;

pub(crate) fn wait(
    join: crate::basis::unit::join::JoinWait,
) -> Result<Handoff<kernel_api::model::unit::JoinReply>, kernel_api::abi::call::unit::UnitFail> {
    loop {
        let _commit = crate::basis::commit();
        let reply = join.poll()?;
        if reply != kernel_api::model::unit::JoinReply::Pending
            || join.remaining() == Duration::ZERO
        {
            return Ok(Handoff::Resume(reply));
        }
        let blocked = block_with_call::<kernel_api::abi::call::unit::UnitFail>(
            join.target.key(),
            join.target.life(),
            join.remaining(),
            Some(WaitCall::Join(join.clone())),
        )?;
        drop(_commit);
        match finish_block(blocked) {
            Handoff::Resume(()) => continue,
            Handoff::Switch(pa) => return Ok(Handoff::Switch(pa)),
        }
    }
}
pub(crate) fn resume(task: &Arc<Task>, join: crate::basis::unit::join::JoinWait) -> Resumption {
    use crate::machine::switcher::context::TrapContext;
    use kernel_api::model::unit::JoinReply;
    let _commit = crate::basis::commit();
    let result = join.poll();
    let result = if matches!(result, Ok(JoinReply::Pending)) && join.remaining() != Duration::ZERO {
        match requeue(
            task,
            join.target.key(),
            join.target.life(),
            join.remaining(),
            WaitCall::Join(join.clone()),
            kernel_api::abi::call::unit::UnitFail::OoM,
        ) {
            Ok(()) => return Resumption::Waiting,
            Err(error) => Err(error),
        }
    } else {
        result
    };
    let frame = unsafe { &mut *(task.ident.frame_pa.as_usize() as *mut TrapContext) };
    crate::basis::unit::join::JoinWait::write(frame, result);
    Resumption::Complete
}
