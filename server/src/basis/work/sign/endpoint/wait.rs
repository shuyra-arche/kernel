use crate::basis::work::room::Handoff;
use crate::basis::work::sign::{block_with_call, finish_block, requeue};
use crate::basis::work::unit::task::{Resumption, Task, WaitCall};
use alloc::sync::Arc;
use core::time::Duration;

pub(crate) fn wait(
    wait: crate::basis::work::sign::endpoint::AwaitWait,
) -> Result<Handoff<kernel_api::abi::reply::SignReply>, kernel_api::abi::call::sign::SignFail> {
    loop {
        let commit = crate::basis::work::commit();
        let reply = match wait.poll() {
            Ok(reply) => reply,
            Err(e) => {
                wait.unwatch();
                return Err(e);
            }
        };
        if reply != kernel_api::abi::reply::SignReply::Pending || wait.remaining() == Duration::ZERO
        {
            wait.unwatch();
            return Ok(Handoff::Resume(reply));
        }
        wait.watch()?;
        let blocked = match block_with_call::<kernel_api::abi::call::sign::SignFail>(
            wait.key(),
            wait.life(),
            wait.remaining(),
            Some(WaitCall::Sign(wait.clone())),
        ) {
            Ok(blocked) => blocked,
            Err(e) => {
                wait.unwatch();
                return Err(e);
            }
        };
        drop(commit);
        match finish_block(blocked) {
            Handoff::Resume(()) => continue,
            Handoff::Switch(pa) => return Ok(Handoff::Switch(pa)),
        }
    }
}
pub(crate) fn resume(
    task: &Arc<Task>,
    wait: crate::basis::work::sign::endpoint::AwaitWait,
) -> Resumption {
    let _commit = crate::basis::work::commit();
    let result = wait.poll();
    let result = if matches!(result, Ok(kernel_api::abi::reply::SignReply::Pending))
        && wait.remaining() != Duration::ZERO
    {
        match requeue(
            task,
            wait.key(),
            wait.life(),
            wait.remaining(),
            WaitCall::Sign(wait.clone()),
            kernel_api::abi::call::sign::SignFail::OoM,
        ) {
            Ok(()) => return Resumption::Waiting,
            Err(e) => Err(e),
        }
    } else {
        result
    };
    let frame = unsafe {
        &mut *(task.ident.frame_pa.as_usize()
            as *mut crate::machine::switcher::context::TrapContext)
    };
    wait.unwatch();
    wait.write(frame, result);
    Resumption::Complete
}
