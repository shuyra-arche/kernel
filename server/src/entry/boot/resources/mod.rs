use crate::basis::work::pend::PendEntry;
use kernel_api::boot::ledger::Name;

mod failure;
mod frozen;
pub(crate) mod ledger;
mod registry;
mod sign;
pub(crate) use sign::{ENTRIES as SIGN_ENTRIES, authority as grant_sign};

use failure::Failure;
pub(crate) use frozen::Frozen;
pub(crate) use registry::Registry;

pub(crate) type RegisterError = Failure<(Name, PendEntry)>;
pub(crate) type FreezeError = Failure<Registry>;
pub(crate) type DeliveryError = Failure<Frozen>;

fn valid(name: Name, pend_entry: &PendEntry) -> bool {
    let mut bytes = [0u8; kernel_api::boot::ledger::NAME_LEN];
    name.store(&mut bytes);
    Name::fetch(&bytes) == Some(name)
        && name.kind() == pend_entry.view().kind().into()
        && pend_entry.alive()
        && pend_entry.sire().is_none()
        && pend_entry.authority().modifiable().is_some()
        && pend_entry.authority().modifiable().is_some()
}

mod control;
pub(crate) use control::{ENTRIES as CONTROL_ENTRIES, authority as grant_controls};
