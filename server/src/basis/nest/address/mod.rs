//! Address placement, span occupancy and reuse within one Nest.

mod layout;
mod occupancy;
mod placement;
mod prepared;
mod range;

pub(crate) use layout::AddressError;
pub(super) use layout::Layout;
pub(crate) use layout::RegionKind;
pub(crate) use placement::{Placement, PlacementAt};
pub(crate) use prepared::PreparedSpan;

pub(crate) mod span;
pub(crate) use range::NestRange;
