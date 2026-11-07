#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    Available,
    Owned,
    Active,
    Retiring,
    Closed,
    Failed,
}

#[derive(Clone, Copy)]
pub(super) enum Event {
    Claim,
    Activate,
    Retire,
    Settle,
    Fail,
}

impl Phase {
    pub(super) fn transition(self, event: Event) -> Option<Self> {
        use Event::*;
        match (self, event) {
            (Self::Available, Claim) => Some(Self::Owned),
            (Self::Owned, Activate) => Some(Self::Active),
            (Self::Owned | Self::Active | Self::Failed | Self::Retiring, Retire) => {
                Some(Self::Retiring)
            }
            (Self::Retiring, Settle) => Some(Self::Closed),
            (Self::Closed, _) => None,
            (_, Fail) => Some(Self::Failed),
            _ => None,
        }
    }
}
