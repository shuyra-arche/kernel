//! The suspended operation and its resume/cancel effects belong to the Task.
use super::Task;
use crate::machine::clock;
use alloc::sync::Arc;

#[derive(Clone)]
pub(crate) enum WaitCall {
    Value { deadline: Option<u64> },
    Join(crate::core::work::unit::join::JoinWait),
    Mail(crate::core::work::mail::tole::AwaitWait),
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
            Self::Join(join) => crate::core::work::unit::join::wait::resume(task, join),
            Self::Mail(wait) => crate::core::work::mail::tole::wait::resume(task, wait),
        }
    }

    pub(crate) fn cancel(self) {
        if let Self::Mail(wait) = self {
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
