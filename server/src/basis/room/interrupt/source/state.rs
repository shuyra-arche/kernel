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
    pub(super) fn rearm(
        &mut self,
        holder: Holder,
        generation: u64,
        sequence: u64,
    ) -> Result<bool, Error> {
        if self.generation != generation {
            return Err(Error::Stale);
        }
        match self.binding {
            Binding::Held {
                holder: known,
                delivery,
            } if known == holder => {
                if !matches!((delivery, sequence), (Delivery::Dormant, 0))
                    && delivery != Delivery::Pending(sequence)
                {
                    return Err(Error::Stale);
                }
                self.binding = Binding::Held {
                    holder,
                    delivery: Delivery::Armed,
                };
                let complete = self.claim == Claim::Held;
                self.claim = Claim::Clear;
                Ok(complete)
            }
            _ => Err(Error::Stale),
        }
    }
    pub(super) fn owned(self, holder: Holder) -> bool {
        matches!(self.binding, Binding::Held { holder: known, .. } if known == holder)
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
    pub(super) fn yield_binding(&mut self, holder: Holder, generation: u64) -> Result<(), Error> {
        if self.generation != generation {
            return Err(Error::Stale);
        }
        if !matches!(self.binding, Binding::Held { holder: known, .. } if known == holder) {
            return Err(Error::Stale);
        }
        self.binding = Binding::Vacant;
        Ok(())
    }
    pub(super) fn revoke(&mut self, permit: PendId) -> bool {
        if matches!(self.binding, Binding::Held { holder, .. } if holder.permit == permit) {
            self.binding = Binding::Vacant;
            true
        } else {
            false
        }
    }
}
