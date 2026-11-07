//! Physical resource attribution, independent of mapping and borrowing handles.
use crate::model::identity::TeamId;

/// A protection domain owns managed memory. Task membership and management
/// authority do not create additional owners of the same physical Page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageOwner {
    Kernel,
    Team(TeamId),
}
