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

impl Phase {
    pub(crate) fn allows(self, next: Self) -> bool {
        use Phase::*;
        matches!(
            (self, next),
            (Held, Ready | Terminating)
                | (Ready, Running | Ready | Parked | Terminating)
                | (Running, Ready | Waiting | Parked | Terminating)
                | (Waiting, Waiting | Ready | Terminating)
                | (Parked, Ready | Terminating)
                | (Terminating, Terminating | Finished)
        )
    }
}
