#[path = "../../../../src/entry/call/next_memory.rs"]
mod next_memory;
use crate::support::*;
use kernel_api::abi::memory::{Action, Failure, Observation, Outcome, Query, Value};
use std::sync::{Arc, atomic::Ordering};
pub(crate) fn invoke(
    frame: &mut TrapContext,
    ident: &Arc<TaskIdent>,
    number: u64,
    registers: [usize; 6],
) {
    next_memory::dispatch(frame, ident.clone(), number, registers);
}
mod memory {
    use super::*;
    pub(super) fn caller(ident: &Arc<TaskIdent>) -> Option<Arc<Task>> {
        ident.task.upgrade()
    }
}
// Explicit effects fixtures. No fixture asserts that a resource permission or
// a hardware completion is valid. Production entry/result code is under test.
mod flat {
    pub(super) mod next {
        use super::super::*;
        pub(crate) fn inspect(_: &Arc<Task>, _: Query) -> Result<Observation, Failure> {
            Err(Failure::Unsupported)
        }
        pub(crate) fn prepare(_: &Arc<Task>, _: Action) -> Result<Option<()>, Failure> {
            Ok(None)
        }
        pub(crate) fn mutate(
            actor: &Arc<Task>,
            _: Action,
            _: Option<()>,
        ) -> Result<Value, Failure> {
            assert_ne!(commit_depth(), 0);
            actor.effects.fetch_add(1, Ordering::SeqCst);
            Ok(Value::Slot {
                version: kernel_api::model::flat::SlotVersion::new(2).unwrap(),
                page: 17,
            })
        }
        pub(crate) fn effect(
            _: &Arc<Task>,
            _: Action,
            _: &mut next_memory::Receipt<'_>,
        ) -> Result<Value, Failure> {
            Err(Failure::Unsupported)
        }
    }
}
mod nest {
    pub(super) mod next {
        use super::super::*;
        pub(crate) fn inspect(_: &Arc<Task>, _: Query) -> Result<Observation, Failure> {
            Err(Failure::Unsupported)
        }
        pub(crate) fn mutate(_: &Arc<Task>, _: Action) -> Result<Value, Failure> {
            Err(Failure::Unsupported)
        }
        pub(crate) fn effect(
            actor: &Arc<Task>,
            _: Action,
            receipt: &mut next_memory::Receipt<'_>,
        ) -> Result<Value, Failure> {
            if actor.mode.load(Ordering::SeqCst) == 1 {
                return Err(Failure::Denied);
            }
            let value = Value::Memo {
                generation: kernel_api::model::nest::MemoGeneration::new(2).unwrap(),
            };
            {
                let _commit = commit();
                receipt.start(value);
            }
            // Stand in for an effect boundary/fence. All three locks must be free.
            assert_eq!(commit_depth(), 0);
            assert!(actor.pend.0.try_lock().is_ok());
            let window = actor.memory_results.0.try_lock().unwrap();
            let journal = window.as_ref().unwrap();
            let id = kernel_api::abi::memory::RequestId::new(journal.high_water()).unwrap();
            assert_eq!(
                journal
                    .inspect(actor.ident.id, kernel_api::abi::memory::Domain::Nest, id)
                    .unwrap()
                    .unwrap()
                    .outcome(),
                Outcome::Pending(value)
            );
            drop(window);
            actor.effects.fetch_add(1, Ordering::SeqCst);
            if actor.mode.load(Ordering::SeqCst) == 2 {
                Err(Failure::Busy)
            } else {
                let _commit = commit();
                assert_eq!(receipt.started(), Some(value));
                receipt.committed(value);
                Ok(value)
            }
        }
    }
}
