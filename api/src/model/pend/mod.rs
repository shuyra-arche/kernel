//! Delegation, authorized ranges and Pend observation.

mod accord;
mod info;
mod permission;
mod range;
mod scope;
mod target;

pub use accord::{AccordRequest, AccordTarget, NarrowRequest};
pub use info::{PendInfo, PendStatus};
pub use permission::{
    CloneRights, CreationRights, Distributable, FlatRights, Limits, NestRights, Operations,
    ShareRights,
};
pub use range::PendRange;
pub use scope::{NestScope, PendScope};
pub use target::{Target, ViewTarget};
