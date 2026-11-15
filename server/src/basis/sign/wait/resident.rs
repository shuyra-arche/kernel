//! A single pre-registered Kernel Task notification. The Sign Site owns the
//! waiter and retained wake; this binding owns no resource or cleanup ledger.
use super::*;
use crate::basis::nest::NestError;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

pub(crate) struct Resident {
    id: AtomicUsize,
    retry: AtomicU64,
}
impl Resident {
    pub(crate) const fn new() -> Self {
        Self {
            id: AtomicUsize::new(0),
            retry: AtomicU64::new(u64::MAX),
        }
    }
    pub(crate) fn bind(&self, task: &Arc<Task>) -> Result<(), NestError> {
        let _commit = crate::basis::commit();
        assert!(Arc::ptr_eq(
            &task.ident.team,
            &crate::basis::unit::team::kernel().expect("Kernel Team")
        ));
        assert!(task.held(), "register before Task release");
        assert_eq!(self.id.load(Ordering::Relaxed), 0);
        let key = WakeKey::Alarm {
            task: task.ident.id,
        };
        Site::ensure(&mut sites(key).lock(), key, &task.life())
            .map_err(|_| NestError::OutOfMemory)?
            .reserve();
        self.id.store(task.ident.id.get(), Ordering::Release);
        Ok(())
    }
    pub(crate) fn identifies(&self, id: usize) -> bool {
        id != 0 && self.id.load(Ordering::Acquire) == id
    }
    pub(crate) fn bound(&self) -> bool {
        self.id.load(Ordering::Acquire) != 0
    }
    pub(crate) fn notify(&self) {
        let _commit = crate::basis::commit();
        let id = self.id.load(Ordering::Acquire);
        if id == 0 {
            return;
        }
        let key = WakeKey::Alarm {
            task: kernel_api::model::identity::TaskId::new(id),
        };
        let chain = sites(key)
            .lock()
            .get_mut(&key)
            .and_then(|site| site.notify(true));
        rise(unchain(chain), insert);
    }
    /// Hardware Busy keeps its obligation with the owner. Only a due timer
    /// notification retries it; callers never spin until hardware completion.
    pub(crate) fn defer(&self) {
        let at = clock::now().add(Duration::from_millis(1)).as_ticks();
        self.retry.fetch_min(at, Ordering::AcqRel);
        crate::basis::room::deadline::retry();
    }
    pub(crate) fn poll(&self) {
        // An unexpectedly cancelled service may never execute its loop epilogue.
        // Resolve only its registered identity, with all Hart-tracked weak borrows
        // consumed under COMMIT; fallback must not depend on that Task returning.
        let dead = {
            let _commit = crate::basis::commit();
            let id = self.id.load(Ordering::Acquire);
            id != 0
                && crate::basis::unit::task::registry::find(
                    kernel_api::model::identity::TaskId::new(id),
                )
                .and_then(|weak| weak.upgrade())
                .is_none_or(|task| task.dead())
        };
        if dead {
            self.close();
        }

        let due = self.retry.load(Ordering::Acquire);
        if due != u64::MAX {
            if clock::now().as_ticks() >= due {
                if self
                    .retry
                    .compare_exchange(due, u64::MAX, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
                {
                    self.notify();
                }
            } else {
                crate::basis::room::deadline::retry();
            }
        }
    }
    /// Called only from the trusted Task trap stack, with the saved continuation
    /// already advanced. Enrollment and producer wake share COMMIT. No timer,
    /// holder, ticket sequence or allocation is needed after bind().
    pub(crate) fn wait(&self) -> Handoff<()> {
        let blocked = {
            let commit = crate::basis::commit();
            #[cfg(debug_assertions)]
            let _no_allocation = crate::metal::allocator::NoAllocation::enter();
            let task = current().running_task().expect("resident Task");
            assert!(self.identifies(task.ident.id.get()));
            let key = WakeKey::Alarm {
                task: task.ident.id,
            };
            let doomed = crate::basis::unit::lifecycle::take_doomed(task.ident.id);
            drop(task);
            if let Some(reason) = doomed {
                block::Blocked::Quit(reason)
            } else if take_beacon(key) {
                block::Blocked::Resume
            } else {
                let (task, next) = current().handoff(
                    crate::basis::room::scheduler::slot::Departure::Wait {
                        key,
                        ticket: Ticket::RESIDENT,
                        call: None,
                    },
                    &commit,
                );
                assert!(
                    sites(key)
                        .lock()
                        .get_mut(&key)
                        .expect("reserved Site")
                        .register(task)
                );
                block::Blocked::Switch(next)
            }
        };
        finish_block(blocked)
    }
    /// Close before issuing Task Exit so scheduler-stack fallback can retire
    /// the cleanup Task itself. No retained service reference delays tally.
    pub(crate) fn close(&self) {
        let _commit = crate::basis::commit();
        let id = self.id.swap(0, Ordering::AcqRel);
        self.retry.store(u64::MAX, Ordering::Release);
        if id != 0 {
            wipe(WakeKey::Alarm {
                task: kernel_api::model::identity::TaskId::new(id),
            });
        }
    }
}
