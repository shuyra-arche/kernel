//! Address placement, reservation and reuse within one Nest.

mod layout;
mod occupancy;
mod placement;
mod reservation;

pub(super) use layout::Layout;
pub(crate) use layout::RegionKind;
pub(crate) use placement::{Placement, PlacementAt};
pub use reservation::Reservation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AddressError {
    OutOfMemory,
    NotAligned,
    NoRegion,
    Occupied,
}
