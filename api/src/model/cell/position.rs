use super::{FlatReference, MappingId};
use crate::model::identity::TeamId;

/// A position within a Cell collection. IDs never confer authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Position {
    Slot {
        reference: FlatReference,
        page: usize,
    },
    Memo {
        team: TeamId,
        mapping: MappingId,
        page: usize,
    },
}
