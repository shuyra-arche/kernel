//! Resource owners unrelated to the Memo state machine are explicit fixtures.
//! They do not model authorization, Task scheduling, COMMIT or hardware fences.
pub(crate) use crate::name;
pub(crate) fn commit() {}
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
    pub(crate) struct ExternalBorrow;
    impl ExternalBorrow {
        pub(crate) unsafe fn settle(self) -> Option<(TaskId, PendId)> {
            panic!("external authority outside Memo fixture")
        }
        pub(crate) fn publish(&mut self) {
            panic!("external authority outside Memo fixture")
        }
    }
    pub(crate) fn notify(_: TaskId, _: PendId) {
        panic!("Task notification outside Memo fixture")
    }
}
