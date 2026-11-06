use crate::core::work::pend::Pie;
use kernel_api::boot::ledger::Name;

mod failure;
mod frozen;
mod guard;
pub(crate) mod ledger;
mod registry;
mod retention;

use failure::Failure;
pub(crate) use frozen::Frozen;
pub(crate) use guard::{ENTRIES as GUARD_ENTRIES, grant as grant_guard};
pub(crate) use registry::Registry;
pub(crate) use retention::{ENTRIES as RETENTION_ENTRIES, grant as grant_retention};

pub(crate) type RegisterError = Failure<(Name, Pie)>;
pub(crate) type FreezeError = Failure<Registry>;
pub(crate) type GrantError = Failure<Frozen>;

fn valid(name: Name, pie: &Pie) -> bool {
    let mut bytes = [0u8; kernel_api::boot::ledger::NAME_LEN];
    name.store(&mut bytes);
    Name::fetch(&bytes) == Some(name)
        && name.kind() == pie.mail().kind()
        && pie.alive()
        && pie.sire().is_none()
        && pie.heir().is_none()
        && !pie.permission().is_empty()
}
