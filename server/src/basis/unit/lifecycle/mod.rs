//! Unit termination, retained results and resource retirement.
//! Event transport is provided by Sign; notifications do not own exit state.

pub(crate) mod diagnostics;
mod doom;
pub(crate) mod join;
mod reap;
pub(crate) mod tally;

use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

use reap::HUSKS;

struct ExitNote {
    reason: AtomicUsize,
    va: AtomicUsize,
    len: AtomicUsize,
}

impl ExitNote {
    const fn new() -> Self {
        Self {
            reason: AtomicUsize::new(0),
            va: AtomicUsize::new(0),
            len: AtomicUsize::new(0),
        }
    }
}

static EXIT: [ExitNote; crate::metal::layout::MAX_HART_SLOTS] =
    [const { ExitNote::new() }; crate::metal::layout::MAX_HART_SLOTS];

fn slot() -> usize {
    crate::metal::hart::hart_id()
        .get()
        .min(crate::metal::layout::MAX_HART_SLOTS - 1)
}

pub(crate) fn set_exit_reason(reason: usize) {
    EXIT[slot()].reason.store(reason, Ordering::Relaxed);
}

fn take_exit_reason() -> usize {
    EXIT[slot()].reason.swap(0, Ordering::Relaxed)
}

pub(crate) fn set_exit_note(va: usize, len: usize) {
    let note = &EXIT[slot()];
    note.va.store(va, Ordering::Relaxed);
    note.len.store(len, Ordering::Relaxed);
}

fn take_exit_note() -> (usize, usize) {
    let note = &EXIT[slot()];
    (
        note.va.swap(0, Ordering::Relaxed),
        note.len.swap(0, Ordering::Relaxed),
    )
}

pub(crate) use kernel_api::model::exit::EXIT_FAULT;

pub(crate) const EXIT_DOOM: usize = 0xFFFF_FFFE;

pub(crate) const EXIT_CASCADE: usize = 0xFFFF_FFFD;

pub(crate) use doom::cull;
pub(crate) use doom::doom;
pub(crate) use doom::exit_team;
pub(crate) use doom::slay;
pub(crate) use doom::take_doomed;
pub(crate) use doom::{Termination, branch_stats};
pub(crate) use reap::bury;
pub(crate) use reap::hook;
pub(crate) use reap::quit;

/// Only for a detached ordinary wait continuation, after its ticket and wait
/// have been removed. Reuse the normal exit/retirement owner without re-detach.
pub(crate) fn fault(task: &alloc::sync::Arc<crate::basis::unit::task::Task>, reason: usize) {
    reap::reap(
        task.clone(),
        kernel_api::model::unit::ExitCause::Fault,
        reason,
    );
}

pub(crate) fn rip() {
    let retired = HUSKS.lock().take();
    drop(retired);
}

#[cfg(debug_assertions)]
pub(crate) fn retained_count() -> usize {
    HUSKS.lock().len()
}
