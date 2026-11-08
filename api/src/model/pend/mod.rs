//! Delegation, authorized ranges and Pend observation.

mod accord;
mod info;
mod permission;
mod range;
mod target;

pub use accord::{AccordRequest, AccordTarget, NarrowRequest};
pub use info::{PendInfo, PendStatus};
pub use permission::{
    CloneRights, CreationRights, Distributable, FlatRights, Limits, NestRights, Operations,
    PortRights, ShareRights,
};
pub use range::PendRange;
pub use target::{Target, ViewTarget};
