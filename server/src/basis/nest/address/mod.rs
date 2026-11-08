//! Address placement, reservation and reuse within one Nest.

mod layout;
mod occupancy;
mod placement;
mod range;
mod reservation;

pub(crate) use layout::AddressError;
pub(super) use layout::Layout;
pub(crate) use layout::RegionKind;
pub(crate) use placement::{Placement, PlacementAt};
pub use reservation::Reservation;

pub(crate) mod span;
pub(crate) use range::NestRange;
