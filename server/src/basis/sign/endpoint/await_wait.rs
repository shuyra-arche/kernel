use alloc::sync::Arc;
use alloc::sync::Weak;
use core::time::Duration;

use kernel_api::abi::call::sign::SignFail;
use kernel_api::abi::reply::SignReply;
use kernel_api::model::sign::SignId;
use kernel_api::model::sign::SignRights;

use crate::basis::sign;
use crate::basis::sign::WakeKey;
use crate::basis::unit::life::Life;
use crate::basis::unit::task::Task;

use super::Endpoint;
use super::key;

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
        .map_err(|_| SignFail::OoM)
    }

    pub fn unwatch(&self) {
        if let Some(actor) = self.actor.upgrade() {
            sign::unforward(inspect_key(&actor, self.token), self.group.id);
        }
    }

    pub fn key(&self) -> WakeKey {
        key(&self.group)
    }

    pub fn life(&self) -> Weak<Life> {
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
