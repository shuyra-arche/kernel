//! One binding owns its receiver and delivery state. A controller claim can
//! outlive that binding. Sign's non-reusing sequence is the receipt identity;
//! a second, unused binding generation would supply no additional validation.
use kernel_api::model::identity::{PendId, TaskId};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Holder {
    pub actor: TaskId,
    pub permit: PendId,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Delivery {
    Armed,
    Pending(u64),
}
enum Binding<R> {
    Vacant,
    Held {
        holder: Holder,
        receiver: R,
        delivery: Delivery,
    },
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Claim {
    Clear,
    Held,
}
pub(super) struct State<R> {
    claim: Claim,
    binding: Binding<R>,
    announce: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Error {
    Busy,
    Stale,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    None,
    Open,
    Complete,
}
impl<R> State<R> {
    pub(super) const fn new() -> Self {
        Self {
            claim: Claim::Clear,
            binding: Binding::Vacant,
            announce: false,
        }
    }
    /// Checked before Sign publication, while COMMIT excludes binding changes.
    pub(super) fn admit(&self) -> Result<(), Error> {
        if self.bound() {
            Err(Error::Busy)
        } else {
            Ok(())
        }
    }
    pub(super) fn attach(
        &mut self,
        holder: Holder,
        receiver: R,
        sequence: u64,
    ) -> Result<(), Error> {
        self.admit()?;
        if sequence == 0 {
            return Err(Error::Stale);
        }
        self.binding = Binding::Held {
            holder,
            receiver,
            delivery: Delivery::Pending(sequence),
        };
        self.announce = true;
        Ok(())
    }
    pub(super) fn receiver(&self) -> Option<&R> {
        match &self.binding {
            Binding::Vacant => None,
            Binding::Held { receiver, .. } => Some(receiver),
        }
    }
    pub(super) fn owned(&self, holder: Holder) -> bool {
        matches!(&self.binding, Binding::Held { holder: known, .. } if *known == holder)
    }
    pub(super) fn bound(&self) -> bool {
        matches!(self.binding, Binding::Held { .. })
    }
    pub(super) fn detach(&mut self) {
        self.binding = Binding::Vacant;
        self.announce = true;
    }
    pub(super) fn armed(&self) -> bool {
        matches!(
            self.binding,
            Binding::Held {
                delivery: Delivery::Armed,
                ..
            }
        )
    }
    /// Retain the physical claim even without a live receiver.
    pub(super) fn claimed(&mut self) -> bool {
        self.claim = Claim::Held;
        self.armed()
    }
    pub(super) fn deliver(&mut self, sequence: u64) -> Result<(), Error> {
        match &mut self.binding {
            Binding::Held { delivery, .. } if *delivery == Delivery::Armed && sequence != 0 => {
                *delivery = Delivery::Pending(sequence);
                self.announce = true;
                Ok(())
            }
            _ => Err(Error::Stale),
        }
    }
    /// Only exact successful consumption resumes hardware; cancellation does not.
    pub(super) fn consume(&mut self, sequence: u64) -> Action {
        if sequence != 0
            && let Binding::Held { delivery, .. } = &mut self.binding
            && *delivery == Delivery::Pending(sequence)
        {
            *delivery = Delivery::Armed;
            return match core::mem::replace(&mut self.claim, Claim::Clear) {
                Claim::Clear => Action::Open,
                Claim::Held => Action::Complete,
            };
        }
        Action::None
    }
    pub(super) fn take_notice(&mut self) -> bool {
        core::mem::take(&mut self.announce)
    }
    pub(super) fn request_notice(&mut self) {
        self.announce = true;
    }
}
