use super::Endpoint;
use super::key;
use crate::basis::room::Handoff;
use crate::basis::sign;
use crate::basis::sign::WakeKey;
use crate::basis::sign::block_with_call;
use crate::basis::sign::finish_block;
use crate::basis::sign::requeue;
use crate::basis::unit::anchor::Anchor;
use crate::basis::unit::task::Resumption;
use crate::basis::unit::task::Task;
use crate::basis::unit::task::WaitCall;
use alloc::sync::Arc;
use alloc::sync::Weak;
use core::time::Duration;
use kernel_api::abi::call::sign::SignFail;
use kernel_api::abi::reply::SignReply;
use kernel_api::model::sign::SignId;
use kernel_api::model::sign::SignRights;

#[derive(Clone)]
pub(crate) struct AwaitWait {
    pub group: Arc<Endpoint>,
    pub token: SignId,
    pub actor: Weak<Task>,
    pub deadline: Option<u64>,
}

fn inspect_key(actor: &Arc<Task>, token: SignId) -> WakeKey {
    WakeKey::SignHandle {
        task: actor.ident.id,
        token: token.get(),
    }
}

impl AwaitWait {
    pub fn new(
        group: Arc<Endpoint>,
        token: SignId,
        actor: &Arc<Task>,
        wait: kernel_api::model::wait::Wait,
    ) -> Self {
        Self {
            group,
            token,
            actor: Arc::downgrade(actor),
            deadline: crate::machine::clock::deadline(wait),
        }
    }

    pub fn watch(&self) -> Result<(), SignFail> {
        let actor = self.actor.upgrade().ok_or(SignFail::Denied)?;
        sign::forward(
            inspect_key(&actor, self.token),
            actor.life(),
            self.group.id,
            self.group.life(),
        )
        .map_err(|_| SignFail::OutOfMemory)
    }

    pub fn unwatch(&self) {
        if let Some(actor) = self.actor.upgrade() {
            sign::unforward(inspect_key(&actor, self.token), self.group.id);
        }
    }

    pub fn key(&self) -> WakeKey {
        key(&self.group)
    }

    pub fn life(&self) -> Weak<Anchor> {
        self.group.life()
    }

    pub fn poll(&self) -> Result<SignReply, SignFail> {
        let actor = self.actor.upgrade().ok_or(SignFail::Denied)?;
        let endpoint = actor.signs.get(self.token, SignRights::LISTEN)?;
        if !Arc::ptr_eq(&endpoint, &self.group) {
            return Err(SignFail::Denied);
        }
        self.group.poll(&actor)
    }

    pub fn write(
        &self,
        frame: &mut crate::machine::switcher::context::TrapContext,
        result: Result<SignReply, SignFail>,
    ) {
        Self::write_result(frame, result);
    }

    pub fn write_result(
        frame: &mut crate::machine::switcher::context::TrapContext,
        result: Result<SignReply, SignFail>,
    ) {
        use crate::machine::switcher::context::Gprs;
        let words = match result {
            Ok(reply) => reply.words(),
            Err(e) => [kernel_api::abi::error::FailCode::code(e) as usize, 0, 0],
        };
        frame.gpr.set_x(Gprs::A0, words[0]);
        frame.gpr.set_x(Gprs::A1, words[1]);
        frame.gpr.set_x(Gprs::A2, words[2]);
    }

    pub fn remaining(&self) -> Duration {
        crate::machine::clock::remaining(self.deadline)
    }
}

unsafe impl Send for AwaitWait {}
unsafe impl Sync for AwaitWait {}

pub(crate) fn wait(
    wait: crate::basis::sign::endpoint::AwaitWait,
) -> Result<Handoff<kernel_api::abi::reply::SignReply>, kernel_api::abi::call::sign::SignFail> {
    loop {
        let commit = crate::basis::commit();
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
    wait: crate::basis::sign::endpoint::AwaitWait,
) -> Resumption {
    let _commit = crate::basis::commit();
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
            kernel_api::abi::call::sign::SignFail::OutOfMemory,
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
