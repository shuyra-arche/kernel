//! Direct-event state. Routing and scheduling are effects of committed changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum State {
    Open { pending: bool },
    Closed,
}
#[derive(Clone, Copy, Debug)]
pub(crate) enum Event {
    Trigger,
    Consume,
    Close,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Rejection {
    Closed,
    Busy,
}
impl State {
    pub(crate) const fn new() -> Self {
        Self::Open { pending: false }
    }
    pub(crate) fn transition(self, event: Event) -> Result<Self, Rejection> {
        match (self, event) {
            (_, Event::Close) => Ok(Self::Closed),
            (Self::Closed, _) => Err(Rejection::Closed),
            (Self::Open { pending: false }, Event::Trigger) => Ok(Self::Open { pending: true }),
            (Self::Open { pending: true }, Event::Consume) => Ok(Self::Open { pending: false }),
            _ => Err(Rejection::Busy),
        }
    }
    pub(crate) fn ready(self) -> bool {
        self == Self::Open { pending: true }
    }
    pub(crate) fn alive(self) -> bool {
        self != Self::Closed
    }
}
