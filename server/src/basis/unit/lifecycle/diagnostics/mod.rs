//! Bounded diagnostics for nonzero exits without a registered result recipient.

use kernel_api::model::exit::Reason;
use kernel_api::model::identity::TaskId;

use crate::metal::sync::SpinLock;

mod ring;
pub(crate) use ring::Entry;
use ring::Ring;

static LEDGER: SpinLock<Ring> = SpinLock::new(Ring::new());

pub(super) fn note(task: TaskId, reason: Reason, note: &str, owner: Option<TaskId>) {
    if reason == 0 || owner.is_some() {
        return;
    }
    let entry = Entry::new(task, reason, note);
    LEDGER.lock().push(entry);
}

pub(crate) fn snapshot() -> ring::Snapshot {
    LEDGER.lock().snapshot()
}
