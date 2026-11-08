//! Retained events use non-wrapping sequence numbers. Observation never consumes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum State {
    Open { pending: bool, sequence: u64 },
    Closed,
}
#[derive(Clone, Copy, Debug)]
pub(crate) enum Event {
    Trigger,
    Consume(u64),
    Close,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Rejection {
    Closed,
    Busy,
    Exhausted,
}
impl State {
    pub(crate) const fn new() -> Self {
        Self::Open {
            pending: false,
            sequence: 0,
        }
    }
    pub(crate) fn transition(self, event: Event) -> Result<Self, Rejection> {
        match (self, event) {
            (_, Event::Close) => Ok(Self::Closed),
            (Self::Closed, _) => Err(Rejection::Closed),
            (
                Self::Open {
                    pending: false,
                    sequence,
                },
                Event::Trigger,
            ) => {
                let sequence = sequence.checked_add(1).ok_or(Rejection::Exhausted)?;
                Ok(Self::Open {
                    pending: true,
                    sequence,
                })
            }
            (
                Self::Open {
                    pending: true,
                    sequence,
                },
                Event::Consume(expected),
            ) if expected == sequence => Ok(Self::Open {
                pending: false,
                sequence,
            }),
            _ => Err(Rejection::Busy),
        }
    }
    pub(crate) fn sequence(self) -> Option<u64> {
        match self {
            Self::Open {
                pending: true,
                sequence,
            } => Some(sequence),
            _ => None,
        }
    }
    pub(crate) fn ready(self) -> bool {
        self.sequence().is_some()
    }
    pub(crate) fn alive(self) -> bool {
        self != Self::Closed
    }
}
