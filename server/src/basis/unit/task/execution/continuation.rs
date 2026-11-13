//! The suspended operation and its resume/cancel effects belong to the Task.
use super::super::Task;
use crate::metal::clock;
use alloc::sync::Arc;

#[derive(Clone)]
pub(crate) enum WaitCall {
    Value { deadline: Option<u64> },
    Sleep { deadline: u64 },
    Sign(crate::basis::sign::endpoint::AwaitWait),
    Fault(crate::basis::nest::fault::wait::Cpu),
    FaultRead(crate::basis::nest::fault::read::Read),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Resumption {
    Complete,
    Waiting,
}

impl WaitCall {
    /// Local admitted waits have pre-existing intrusive storage and never
    /// allocate a global ticket, Site, forwarding edge or heap registration.
    pub(crate) fn complete_local(&self, task: &Arc<Task>, now: u64) -> bool {
        match self {
            Self::Sleep { deadline } => now >= *deadline,
            Self::Sign(wait) => {
                let result = wait.poll();
                if matches!(result, Ok(kernel_api::abi::reply::SignReply::Pending))
                    && wait.deadline.is_none_or(|at| now < at)
                {
                    return false;
                }
                let frame = unsafe {
                    &mut *(task.ident.frame_pa.as_usize()
                        as *mut crate::metal::switcher::context::TrapContext)
                };
                wait.unwatch();
                wait.write(frame, result);
                true
            }
            Self::Value { .. } | Self::Fault(_) | Self::FaultRead(_) => {
                unreachable!("wait is outside admitted calls")
            }
        }
    }
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
                            as *mut crate::metal::switcher::context::TrapContext)
                    };
                    frame.gpr.set_x(
                        crate::metal::switcher::context::Gprs::A0,
                        (-2isize) as usize,
                    );
                }
                Resumption::Complete
            }
            Self::Value { deadline } => {
                use crate::metal::switcher::context::{Gprs, TrapContext};
                let timely = deadline.is_none_or(|at| clock::now().as_ticks() < at);
                let frame = unsafe { &mut *(task.ident.frame_pa.as_usize() as *mut TrapContext) };
                frame.gpr.set_x(Gprs::A0, timely as usize);
                Resumption::Complete
            }
            Self::Sign(wait) => crate::basis::sign::endpoint::wait::resume(task, wait),
            Self::Fault(wait) => crate::basis::nest::fault::wait::resume(task, wait),
            Self::FaultRead(wait) => crate::basis::nest::fault::read::resume(task, wait),
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
            Self::Fault(wait) => Some(wait.deadline()),
            Self::FaultRead(wait) => Some(wait.deadline()),
        }
    }
}
