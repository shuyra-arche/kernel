//! The suspended operation and its resume/cancel effects belong to the Task.
use super::Task;
use crate::machine::clock;
use alloc::sync::Arc;

#[derive(Clone)]
pub(crate) enum WaitCall {
    Value { deadline: Option<u64> },
    Join(crate::basis::work::unit::join::JoinWait),
    Sign(crate::basis::work::sign::endpoint::AwaitWait),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Resumption {
    Complete,
    Waiting,
}

impl WaitCall {
    pub(crate) fn resume(self, task: &Arc<Task>) -> Resumption {
        match self {
            Self::Value { deadline } => {
                use crate::machine::switcher::context::{Gprs, TrapContext};
                let timely = deadline.is_none_or(|at| clock::now().as_ticks() < at);
                let frame = unsafe { &mut *(task.ident.frame_pa.as_usize() as *mut TrapContext) };
                frame.gpr.set_x(Gprs::A0, timely as usize);
                Resumption::Complete
            }
            Self::Join(join) => crate::basis::work::unit::join::wait::resume(task, join),
            Self::Sign(wait) => crate::basis::work::sign::endpoint::wait::resume(task, wait),
        }
    }

    pub(crate) fn cancel(self) {
        if let Self::Sign(wait) = self {
            wait.unwatch();
        }
    }

    pub(crate) fn deadline(&self, duration: core::time::Duration) -> Option<u64> {
        match self {
            Self::Value { deadline } => *deadline,
            _ => (duration != core::time::Duration::MAX)
                .then(|| clock::now().add(duration).as_ticks()),
        }
    }
}
