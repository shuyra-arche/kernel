//! Pure result reduction; physical state transitions belong to the Memo owner.
use kernel_api::abi::memory::{Failure, Opcode, Outcome, Value};

pub(crate) enum Transition {
    Published,
    Retired,
    Failed(Failure),
}
impl Transition {
    pub(crate) fn outcome(self, opcode: Opcode, value: Value) -> Outcome {
        match (opcode, self) {
            (Opcode::Nest(kernel_api::abi::memory::nest::Opcode::Publish), Self::Published)
            | (Opcode::Nest(kernel_api::abi::memory::nest::Opcode::Retract), Self::Retired) => {
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
