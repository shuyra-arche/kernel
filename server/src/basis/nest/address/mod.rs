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

/// Address organization shared by a Nest's admission and Span lookup.
/// The translation no longer owns this index once the two object boundaries
/// are separated: occupancy and Span location stay with the address owner.
pub(crate) struct AddressState {
    pub(crate) layout: Layout,
    pub(in crate::basis::nest) index: super::index::Index,
    pub(in crate::basis::nest) team: Option<kernel_api::model::identity::TeamId>,
}

impl core::ops::Deref for AddressState {
    type Target = Layout;
    fn deref(&self) -> &Self::Target {
        &self.layout
    }
}
impl core::ops::DerefMut for AddressState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.layout
    }
}

impl AddressState {
    pub(crate) fn new(layout: Layout) -> Self {
        Self {
            layout,
            index: super::index::Index::new(),
            team: None,
        }
    }
}
