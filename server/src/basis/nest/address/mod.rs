//! Address selection and reservations within one Nest.

mod occupancy;
mod placement;
mod reservation;

pub(crate) use occupancy::RegionId;
pub(super) use occupancy::{AddressError, Occupancy};
pub(crate) use placement::{Placement, PlacementAt};
pub use reservation::Reservation;
