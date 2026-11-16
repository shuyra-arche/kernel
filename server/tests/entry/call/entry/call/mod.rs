#[path = "../../../../../src/entry/call/next_memory.rs"]
mod next_memory;
use crate::support::*;
use kernel_api::abi::memory::{
    Action, Failure, Observation, Query, Value, flat::Action as FlatAction,
    flat::Query as FlatQuery, nest::Action as NestAction, nest::Query as NestQuery,
};
use std::sync::{Arc, atomic::Ordering};
pub(crate) fn invoke(
    frame: &mut TrapContext,
    ident: &Arc<TaskIdent>,
    number: u64,
    registers: [usize; 6],
) {
    crate::metal::sync::boundary(|root| {
        next_memory::dispatch(frame, ident.clone(), number, registers, root)
    });
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
        pub(crate) fn inspect(
            _: &mut crate::basis::sync::Root<'_>,
            _: &Arc<Task>,
            _: FlatQuery,
        ) -> Result<Observation, Failure> {
            Err(Failure::Unsupported)
        }
        pub(crate) fn prepare(_: &Arc<Task>, _: FlatAction) -> Result<Option<()>, Failure> {
            Ok(None)
        }
        pub(crate) fn mutate(
            actor: &Arc<Task>,
            _: FlatAction,
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
            _: FlatAction,
            _: &mut next_memory::Receipt<'_>,
        ) -> Result<Value, Failure> {
            Err(Failure::Unsupported)
        }
    }
}
mod nest {
    pub(super) mod next {
        use super::super::*;
        pub(crate) fn inspect(_: &Arc<Task>, _: NestQuery) -> Result<Observation, Failure> {
            Err(Failure::Unsupported)
        }
        pub(crate) fn mutate(_: &Arc<Task>, _: NestAction) -> Result<Value, Failure> {
            Err(Failure::Unsupported)
        }
        pub(crate) fn effect(
            actor: &Arc<Task>,
            action: NestAction,
            receipt: &mut next_memory::Receipt<'_>,
        ) -> Result<Value, Failure> {
            if actor.mode.load(Ordering::SeqCst) == 1 {
                return Err(Failure::Denied);
            }
            if matches!(action, NestAction::Create { .. }) {
                // Private address-space preparation may fence. The dispatcher
                // must release admission locks before entering this boundary.
                assert_eq!(commit_depth(), 0);
                assert!(actor.pend.0.try_lock().is_ok());
                actor.effects.fetch_add(1, Ordering::SeqCst);
                let value = Value::CreatedNest {
                    token: kernel_api::model::identity::PendId::mint(7),
                    nest: kernel_api::model::nest::NestId::new(8).unwrap(),
                    version: kernel_api::model::nest::NestVersion::INITIAL,
                };
                let _commit = commit();
                receipt.committed(value);
                return Ok(value);
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
            assert_eq!(receipt.started(), Some(value));
            // Read the actual copied descriptor and query through the Task
            // owner while the hardware effect is outside every commit guard.
            let bytes = actor.ident.team.nest.bytes.lock().unwrap();
            let request = kernel_api::abi::memory::Request::from_bytes(
                &bytes[..kernel_api::abi::memory::Request::WIDTH],
            )
            .unwrap();
            drop(bytes);
            let query = kernel_api::abi::memory::Request::new(
                None,
                Action::InspectResult {
                    domain: kernel_api::abi::memory::Domain::Nest,
                    request: request.id().unwrap(),
                },
            )
            .unwrap();
            let crate::request::Admission::Replay(reply) = actor.admit_memory(query).unwrap()
            else {
                panic!("query cannot admit another execution");
            };
            assert_eq!(
                reply.outcome(),
                kernel_api::abi::memory::Outcome::Pending(value)
            );
            actor.effects.fetch_add(1, Ordering::SeqCst);
            if actor.mode.load(Ordering::SeqCst) == 3 {
                *actor.completion.lock() = Some(receipt.defer());
                Ok(value)
            } else if actor.mode.load(Ordering::SeqCst) == 2 {
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
