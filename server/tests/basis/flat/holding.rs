//! Test-owned Flat holding constructors, shared with the Nest handoff fixture.
use crate::{
    cell::{Owner, Page},
    state::{FlatSource, FlatState},
    support::Pool,
};
use std::sync::Arc;

pub(crate) fn fresh() -> FlatState {
    let mut state = FlatState::new(FlatSource::Managed(Vec::new()), true);
    state.explicit = true;
    state
}
pub(crate) fn page(pool: &'static Pool) -> Page {
    Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap()
}
pub(crate) fn resident(state: &FlatState) -> Arc<Page> {
    let FlatSource::Managed(slots) = &state.source else {
        unreachable!()
    };
    let crate::slot::Slot::Resident(page) = &slots[0].1 else {
        unreachable!()
    };
    page.clone()
}
