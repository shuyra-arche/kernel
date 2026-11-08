// Import the production address algorithms without target-only Nest machinery.
#[path = "../../src/basis/nest/address/layout.rs"]
mod layout;
#[path = "../../src/basis/nest/address/occupancy.rs"]
mod occupancy;
#[path = "../../src/basis/nest/address/placement.rs"]
mod placement;
pub(crate) use layout::{AddressError, Layout, RegionKind};
pub(crate) use placement::Placement;
