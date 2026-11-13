//! Pure result reduction; physical state transitions belong to the Memo owner.
use kernel_api::abi::memory::{Action, Failure, Outcome, Value};

pub(crate) enum Transition {
    Published,
    Retired,
    Failed(Failure),
}
impl Transition {
    pub(crate) fn outcome(self, action: Action, value: Value) -> Outcome {
        match (action, self) {
            (Action::Publish { .. }, Self::Published) | (Action::Retract { .. }, Self::Retired) => {
                Outcome::Committed(value)
            }
            (_, Self::Failed(failure)) => Outcome::Failed {
                failure,
                retained: Some(value),
            },
            _ => Outcome::Failed {
                failure: Failure::Denied,
                retained: Some(value),
            },
        }
    }
}
