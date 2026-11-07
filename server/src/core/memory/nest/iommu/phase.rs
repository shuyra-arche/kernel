use super::{BatchId, Error};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    Prepared,
    Active,
    Revoking,
    Waiting(BatchId),
    Failed,
    Settled,
}

#[derive(Clone, Copy)]
pub(super) enum Event {
    Publish,
    Revoke,
    Submitted(BatchId),
    Complete(BatchId),
    Fail,
}

impl Phase {
    pub(super) fn transition(self, event: Event) -> Result<Self, Error> {
        match (self, event) {
            (Self::Prepared, Event::Publish) => Ok(Self::Active),
            (Self::Prepared, Event::Revoke) => Ok(Self::Settled),
            (Self::Active | Self::Failed, Event::Revoke) => Ok(Self::Revoking),
            (Self::Revoking, Event::Submitted(batch)) => Ok(Self::Waiting(batch)),
            (Self::Waiting(expected), Event::Complete(seen)) if expected == seen => {
                Ok(Self::Settled)
            }
            (Self::Waiting(_), Event::Complete(_)) => Err(Error::WrongBatch),
            (Self::Active | Self::Revoking | Self::Waiting(_), Event::Fail) => Ok(Self::Failed),
            _ => Err(Error::InvalidState),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Life {
    Open,
    Closing,
    Closed,
}

impl Life {
    pub(super) fn close(self, empty: bool) -> Self {
        if empty { Self::Closed } else { Self::Closing }
    }
}
