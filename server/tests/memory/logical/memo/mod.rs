//! Actual Memo and Page state, with inert logical-owner and route fixtures.
//! No fixture constructs a permission or supplies physical completion evidence.
use crate::basis::{nest::authority::RangeBinding, pend::Pend};
pub(crate) use crate::memo as publication;
use kernel_api::model::identity::{PendId, TaskId};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

pub(crate) mod source {
    use super::*;
    #[derive(Clone)]
    pub(crate) struct Source(pub Arc<Registry>);
    impl Source {
        pub(crate) fn position(&self) -> (kernel_api::model::flat::FlatId, usize) {
            (kernel_api::model::flat::FlatId::new(1).unwrap(), 0)
        }
        pub(crate) fn source(&self) -> &Arc<Registry> {
            &self.0
        }
    }
    pub(crate) struct Registry(pub AtomicUsize);
    impl Registry {
        pub(crate) fn release_binding(&self, _: &Arc<RangeBinding>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
}
pub(crate) mod publisher {
    use super::*;
    pub(crate) struct Principal;
    pub(crate) struct Publisher {
        pub settled: Arc<AtomicUsize>,
    }
    impl Publisher {
        pub(crate) fn authority(&self) -> &Arc<Pend> {
            panic!("authority outside Memo fixture")
        }
        pub(crate) fn observe(&self) -> Principal {
            panic!("authority outside Memo fixture")
        }
        pub(crate) fn publish(&mut self) {
            panic!("authority outside Memo fixture")
        }
        pub(crate) unsafe fn settle(self) -> (TaskId, PendId) {
            self.settled.fetch_add(1, Ordering::SeqCst);
            (TaskId::new(1), PendId::mint(1))
        }
    }
}
use publisher::Publisher;
mod cases;
#[path = "../../../../src/basis/nest/memo/state.rs"]
mod state;
