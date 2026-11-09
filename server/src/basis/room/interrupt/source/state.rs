//! A hardware source has one binding and one retained event. Neither generation
//! nor sequence wraps; stale acknowledgements cannot affect a new driver.
use kernel_api::model::identity::{PendId, TaskId};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Holder {
    pub actor: TaskId,
    pub permit: PendId,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Delivery {
    Dormant,
    Armed,
    Pending(u64),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Binding {
    Vacant,
    Held { holder: Holder, delivery: Delivery },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Claim {
    Clear,
    Held,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct State {
    claim: Claim,
    generation: u64,
    binding: Binding,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Error {
    Busy,
    Stale,
    Exhausted,
}
impl State {
    pub(super) const fn new() -> Self {
        Self {
            claim: Claim::Clear,
            generation: 0,
            binding: Binding::Vacant,
        }
    }
    pub(super) fn claim(&mut self, holder: Holder) -> Result<u64, Error> {
        if self.binding != Binding::Vacant {
            return Err(Error::Busy);
        }
        self.generation = self.generation.checked_add(1).ok_or(Error::Exhausted)?;
        self.binding = Binding::Held {
            holder,
            delivery: Delivery::Dormant,
        };
        Ok(self.generation)
    }
    /// The initial inspection and each hardware event use real Sign sequences.
    pub(super) fn begin(&mut self, sequence: u64) {
        if let Binding::Held { delivery, .. } = &mut self.binding {
            *delivery = Delivery::Pending(sequence);
        }
    }
    /// A successful Sign consumption, never cancellation, resumes this producer.
    pub(super) fn consume(&mut self, sequence: u64) -> Option<bool> {
        if sequence == 0 {
            return None;
        }
        if let Binding::Held { delivery, .. } = &mut self.binding {
            if *delivery == Delivery::Pending(sequence) {
                *delivery = Delivery::Armed;
                let held = self.claim == Claim::Held;
                self.claim = Claim::Clear;
                return Some(held);
            }
        }
        None
    }
    pub(super) fn owned(self, holder: Holder) -> bool {
        matches!(self.binding, Binding::Held { holder: known, .. } if known == holder)
    }
    pub(super) fn bound(&self) -> bool {
        self.binding != Binding::Vacant
    }
    pub(super) fn close(&mut self) {
        self.binding = Binding::Vacant;
    }
    pub(super) fn armed(self) -> bool {
        matches!(
            self.binding,
            Binding::Held {
                delivery: Delivery::Armed,
                ..
            }
        )
    }
    pub(super) fn claimed(&mut self) -> bool {
        self.claim = Claim::Held;
        self.armed()
    }
    pub(super) fn deliver(&mut self, sequence: u64) -> Result<(), Error> {
        match self.binding {
            Binding::Held {
                holder,
                delivery: Delivery::Armed,
            } if sequence != 0 => {
                self.binding = Binding::Held {
                    holder,
                    delivery: Delivery::Pending(sequence),
                };
                Ok(())
            }
            _ => Err(Error::Stale),
        }
    }
}
