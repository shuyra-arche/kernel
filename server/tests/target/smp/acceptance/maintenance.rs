use super::worker::Worker;
use super::*;
use core::fmt::Write;
use kernel_api::model::flat::*;

fn credential(response: Response, token: PendId) -> Maintenance {
    let Response::Reply(r) = response else {
        panic!("Block")
    };
    let (Outcome::Committed(Value::Maintenance { block, .. })
    | Outcome::Pending(Value::Maintenance { block, .. })) = r.outcome()
    else {
        panic!("{:?}", r.outcome())
    };
    Maintenance { token, block }
}
pub fn run() {
    for revoked in [false, true] {
        let slot = create(1);
        mutate(Action::Occupy {
            slot,
            expected: slot_status(slot).version,
        });
        let sibling = derive(slot.token(), pend_info(slot.token()).limits);
        let worker = Worker::new();
        let token = accord(slot.token(), worker.0, pend_info(slot.token()).limits);
        let start = SlotPosition::new(token, slot.flat(), 0).unwrap();
        let (memo, _) = reserve(space());
        insert(memo, slot);
        let target = accord(memo.token(), worker.0, pend_info(memo.token()).limits);
        let local = at(memo, target, 0);
        let Value::CreatedNest {
            token: foreign_space,
            ..
        } = mutate(Action::CreateNest { license: license() })
        else {
            panic!("Nest")
        };
        let (foreign_memo, _) = reserve(foreign_space);
        insert(foreign_memo, slot);
        let foreign_target = accord(
            foreign_memo.token(),
            worker.0,
            pend_info(foreign_memo.token()).limits,
        );
        let held = credential(
            worker
                .call(Action::Block {
                    start,
                    count: 1,
                    recover: None,
                })
                .unwrap(),
            token,
        );
        // Even valid source/target grants cannot publish a maintenance use
        // into an Internal Nest other than the maintenance Task's Team.
        rejected(worker.call(Action::Publish {
            memo: at(foreign_memo, foreign_target, 0),
            expected: memo_status(foreign_memo).generation,
            maintenance: Some(held),
        }));
        publish_held(&worker, local, memo, held);
        assert_eq!(slot_status(slot).obligations, 1);
        // A credential for this Flat cannot admit an unrelated source.
        let other = create(1);
        mutate(Action::Occupy {
            slot: other,
            expected: slot_status(other).version,
        });
        let (unrelated, _) = reserve(space());
        insert(unrelated, other);
        let foreign = accord(
            unrelated.token(),
            worker.0,
            pend_info(unrelated.token()).limits,
        );
        rejected(worker.call(Action::Publish {
            memo: at(unrelated, foreign, 0),
            expected: memo_status(unrelated).generation,
            maintenance: Some(held),
        }));
        // A later overlapping Block drains the exact earlier maintenance use.
        let overlap = credential(
            call(Action::Block {
                start: slot,
                count: 1,
                recover: None,
            }),
            slot.token(),
        );
        assert_eq!(slot_status(slot).obligations, 0);
        rejected(worker.call(Action::Publish {
            memo: local,
            expected: memo_status(memo).generation,
            maintenance: Some(held),
        }));
        mutate(Action::Allow {
            maintenance: overlap,
        });
        publish_held(&worker, local, memo, held);
        if revoked {
            let deadline = kernel_client::call::room::clock() + 5_000_000_000;
            loop {
                match pend::revoke(worker.0, token) {
                    Ok(()) => break,
                    Err(error) => assert_eq!(error.into_source(), kernel_api::abi::call::pend::PendFail::Busy),
                }
                assert!(kernel_client::call::room::clock() < deadline);
                kernel_client::call::room::yield_();
            }
            rejected(worker.call(Action::Allow { maintenance: held }));
        }
        worker.stop();
        // Exited is not reclamation: maintenance retirement can still be in
        // flight. The orphan keeps admission closed until ancestor recovery.
        assert!(slot_status(slot).blocked);
        rejected(try_call(Action::Publish {
            memo,
            expected: memo_status(memo).generation,
            maintenance: None,
        }));
        rejected(try_call(Action::Block {
            start: SlotPosition::new(sibling, slot.flat(), 0).unwrap(),
            count: 1,
            recover: Some(held.block),
        }));
        let recovered = credential(
            call(Action::Block {
                start: slot,
                count: 1,
                recover: Some(held.block),
            }),
            slot.token(),
        );
        assert_eq!(slot_status(slot).obligations, 0);
        assert_eq!(recovered.block.record(), held.block.record());
        assert!(recovered.block.generation() > held.block.generation());
        rejected(try_call(Action::Allow {
            maintenance: Maintenance {
                token: slot.token(),
                block: held.block,
            },
        }));
        rejected(try_call(Action::Block {
            start: slot,
            count: 1,
            recover: Some(held.block),
        }));
        mutate(Action::Allow {
            maintenance: recovered,
        });
        publish(memo);
        retract(memo);
    }
    takeover_race();
    let _ = writeln!(
        Console,
        "PASS acceptance-maintenance wrong-source=reject overlap=drained service-death=blocked revoked-source=reject ancestor-recovery=new-epoch sibling-takeover=reject old-credential=reject concurrent-takeover=one-winner"
    );
}

fn takeover_race() {
    for _ in 0..4 {
        let slot = create(1);
        let manager = Worker::new();
        let service = Worker::new();
        let limits = pend_info(slot.token()).limits;
        let parent = accord(slot.token(), manager.0, limits);
        let child = manager.accord(parent, service.0, limits);
        let original = credential(
            service
                .call(Action::Block {
                    start: SlotPosition::new(child, slot.flat(), 0).unwrap(),
                    count: 1,
                    recover: None,
                })
                .unwrap(),
            child,
        );
        service.stop();
        manager.begin(Action::Block {
            start: SlotPosition::new(parent, slot.flat(), 0).unwrap(),
            count: 1,
            recover: Some(original.block),
        });
        let root = call(Action::Block {
            start: slot,
            count: 1,
            recover: Some(original.block),
        });
        let other = manager.finish().unwrap();
        let committed = |response: Response| matches!(response,Response::Reply(r) if matches!(r.outcome(),Outcome::Committed(_)));
        assert_ne!(
            committed(root),
            committed(other),
            "exactly one ancestor takes over"
        );
        if committed(root) {
            rejected(Ok(other));
            let current = credential(root, slot.token());
            assert_eq!(
                current.block.generation(),
                original.block.generation().next().unwrap()
            );
            mutate(Action::Allow {
                maintenance: current,
            });
        } else {
            rejected(Ok(root));
            let current = credential(other, parent);
            assert_eq!(
                current.block.generation(),
                original.block.generation().next().unwrap()
            );
            assert!(committed(
                manager
                    .call(Action::Allow {
                        maintenance: current
                    })
                    .unwrap()
            ));
        }
        manager.stop();
    }
}

fn publish_held(worker: &Worker, local: MemoPosition, observed: MemoPosition, held: Maintenance) {
    let deadline = kernel_client::call::room::clock() + 5_000_000_000;
    loop {
        let Response::Reply(reply) = worker.call(Action::Publish {
            memo: local, expected: memo_status(observed).generation, maintenance: Some(held),
        }).unwrap() else { panic!("Publish") };
        match reply.outcome() {
            Outcome::Committed(_) => return,
            Outcome::Rejected(Failure::Busy | Failure::Stale) => {},
            other => panic!("maintenance Publish: {other:?}"),
        }
        assert!(kernel_client::call::room::clock() < deadline);
        kernel_client::call::room::yield_();
    }
}
