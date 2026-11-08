//! The suspended operation and its resume/cancel effects belong to the Task.
use super::Task;
use crate::machine::clock;
use alloc::sync::Arc;

#[derive(Clone)]
pub(crate) enum WaitCall {
    Value { deadline: Option<u64> },
    Sleep { deadline: u64 },
    Sign(crate::basis::sign::endpoint::AwaitWait),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Resumption {
    Complete,
    Waiting,
}

impl WaitCall {
    pub(crate) fn resume(self, task: &Arc<Task>) -> Resumption {
        match self {
            Self::Sleep { deadline } => {
                if clock::now().as_ticks() < deadline {
                    let key = crate::basis::sign::WakeKey::Alarm {
                        task: task.ident.id,
                    };
                    if crate::basis::sign::requeue(
                        task,
                        key,
                        task.life(),
                        clock::remaining(Some(deadline)),
                        Self::Sleep { deadline },
                        (),
                    )
                    .is_ok()
                    {
                        return Resumption::Waiting;
                    }
                    let frame = unsafe {
                        &mut *(task.ident.frame_pa.as_usize()
                            as *mut crate::machine::switcher::context::TrapContext)
                    };
                    frame.gpr.set_x(
                        crate::machine::switcher::context::Gprs::A0,
                        (-2isize) as usize,
                    );
                }
                Resumption::Complete
            }
            Self::Value { deadline } => {
                use crate::machine::switcher::context::{Gprs, TrapContext};
                let timely = deadline.is_none_or(|at| clock::now().as_ticks() < at);
                let frame = unsafe { &mut *(task.ident.frame_pa.as_usize() as *mut TrapContext) };
                frame.gpr.set_x(Gprs::A0, timely as usize);
                Resumption::Complete
            }
            Self::Sign(wait) => crate::basis::sign::endpoint::wait::resume(task, wait),
        }
    }

    pub(crate) fn cancel(self) {
        if let Self::Sign(wait) = self {
            wait.unwatch();
        }
    }

    pub(crate) fn deadline(&self, _duration: core::time::Duration) -> Option<u64> {
        match self {
            Self::Value { deadline } => *deadline,
            Self::Sleep { deadline } => Some(*deadline),
            Self::Sign(wait) => wait.deadline,
        }
    }
}
