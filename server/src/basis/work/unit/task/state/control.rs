//! Pure execution-control decisions. Queue, hart and notification effects are
//! applied by Task while holding the work commit boundary.

use crate::basis::work::unit::control::Request;
use kernel_api::abi::call::unit::UnitFail;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    Held,
    Ready,
    Running,
    Waiting,
    Parked,
    Terminating,
    Finished,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Control {
    pub phase: Phase,
    pub paused: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Effect {
    None,
    Release,
    Enqueue,
    Dequeue,
    Interrupt,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Transition {
    pub before: Control,
    pub after: Control,
    pub effect: Effect,
}

impl Control {
    /// Compute the next state and required external action without changing
    /// either the Task or its execution environment.
    pub(crate) fn transition(self, request: Request) -> Result<Transition, UnitFail> {
        let (after, effect) = match request {
            Request::Pause => match self.phase {
                Phase::Terminating | Phase::Finished => return Err(UnitFail::Denied),
                Phase::Running if self.paused => return Err(UnitFail::Busy),
                _ if self.paused => (self, Effect::None),
                Phase::Running => (
                    Self {
                        paused: true,
                        ..self
                    },
                    Effect::Interrupt,
                ),
                Phase::Ready => (
                    Self {
                        paused: true,
                        ..self
                    },
                    Effect::Dequeue,
                ),
                Phase::Parked => (
                    Self {
                        phase: Phase::Ready,
                        paused: true,
                    },
                    Effect::None,
                ),
                Phase::Held | Phase::Waiting => (
                    Self {
                        paused: true,
                        ..self
                    },
                    Effect::None,
                ),
            },
            Request::Resume => {
                if !self.paused {
                    return Err(UnitFail::Busy);
                }
                match self.phase {
                    Phase::Terminating | Phase::Finished => return Err(UnitFail::Denied),
                    Phase::Waiting => (
                        Self {
                            paused: false,
                            ..self
                        },
                        Effect::None,
                    ),
                    Phase::Held => (
                        Self {
                            paused: false,
                            ..self
                        },
                        Effect::Release,
                    ),
                    Phase::Ready => (
                        Self {
                            paused: false,
                            ..self
                        },
                        Effect::Enqueue,
                    ),
                    Phase::Running | Phase::Parked => return Err(UnitFail::Busy),
                }
            }
        };
        Ok(Transition {
            before: self,
            after,
            effect,
        })
    }
}
