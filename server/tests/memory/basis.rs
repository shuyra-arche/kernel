//! Resource owners unrelated to the Memo state machine are explicit fixtures.
//! They do not model authorization, Task scheduling, COMMIT or hardware fences.
pub(crate) use crate::name;
pub(crate) fn commit() {}
pub(crate) mod unit {
    pub(crate) mod task {
        pub(crate) mod memory_result {
            #[derive(Debug, PartialEq, Eq)]
            pub(crate) enum Transition {
                Published,
                Retired,
            }
            pub(crate) struct Completion(pub std::sync::Arc<std::sync::Mutex<Vec<Transition>>>);
            impl Completion {
                pub(crate) fn finish(self, transition: Transition) {
                    // Observe the Memo ownership transfer; Task journal policy is
                    // covered by the invocation tests, not manufactured here.
                    self.0.lock().unwrap().push(transition);
                }
            }
        }
    }
}
pub(crate) mod cell {
    pub(crate) use crate::cell as page;
    pub(crate) use crate::changes as change;
}
pub(crate) mod flat {
    pub(crate) use crate::error::FlatError;
    pub(crate) mod transfer {
        pub(crate) use crate::transfer_error::TransferError;
    }
}
#[path = "logical/mod.rs"]
pub(crate) mod nest;
pub(crate) mod pend {
    use kernel_api::model::identity::{PendId, TaskId};
    pub(crate) struct Pend;
    // Opaque native owner references. These fixtures never authorize a transfer
    // or publication; the physical/storage tests exercise the production gate.
    pub(crate) struct Stage;
    pub(crate) struct Provenance;
    pub(crate) struct MaintenanceUse;
    pub(crate) struct ExternalBorrow;
    impl ExternalBorrow {
        pub(crate) unsafe fn settle(self) -> Option<(TaskId, PendId)> {
            panic!("external authority outside Memo fixture")
        }
        pub(crate) fn publish(&mut self) {
            panic!("external authority outside Memo fixture")
        }
    }
    pub(crate) fn notify(task: TaskId, token: PendId) {
        // The inert Publisher fixture reports settlement, not live authority.
        assert_eq!((task.get(), token.get()), (1, 1));
    }
}
