use super::worker::Worker;
use super::*;
use core::fmt::Write;
use kernel_api::model::{cell::Placement, flat::*, modifiable::Modifiable};
use kernel_client::call::share_view;

fn share_from(slot: SlotPosition, writable: bool, distributable: Distributable) -> PendId {
    let bytes = AccordRequest {
        src: slot.token(),
        target: AccordTarget::Caller,
        limits: Limits {
            operations: Operations::Share(ShareRights::ASSOCIATE),
            modifiable: if writable {
                Modifiable::Yes
            } else {
                Modifiable::No
            },
            distributable,
            scope: PendScope::Pages(PendRange::Inherit),
        },
        mark: Mark::NONE,
    }
    .to_bytes();
    share_view::derive(license(), VirtAddr::new(bytes.as_ptr() as usize)).unwrap()
}
fn share_source(token: PendId) -> Source {
    let Target::View(ViewTarget::Share(view)) = pend_info(token).target else {
        panic!("Share")
    };
    Source::Share {
        token,
        view,
        page: 0,
        count: 1,
    }
}
fn associate(target: MemoPosition, source: Source) -> Action {
    Action::Insert {
        target,
        expected: memo_status(target).generation,
        source,
    }
}
pub fn run() {
    derivation_controls();
    let slot = create(1);
    mutate(Action::Occupy {
        slot,
        expected: slot_status(slot).version,
    });
    let readonly = share_from(slot, false, Distributable::Yes);
    let (first, _) = reserve_at(space(), Placement::lowest(4096).unwrap(), 2);
    mutate(associate(first, share_source(readonly)));
    publish(first);
    let (second, _) = reserve_at(space(), Placement::lowest(4096).unwrap(), 2);
    mutate(associate(
        second,
        Source::Memo {
            position: first,
            expected: memo_status(first).generation,
        },
    ));
    publish(second);
    assert_eq!(slot_status(slot).obligations, 2);
    let (writable, _) = reserve(space());
    mutate(associate(
        writable,
        Source::Memo {
            position: second,
            expected: memo_status(second).generation,
        },
    ));
    rejected(try_call(Action::Publish {
        memo: writable,
        expected: memo_status(writable).generation,
        maintenance: None,
    }));
    rejected(try_call(Action::Protect {
        memo: second,
        expected: memo_status(second).generation,
        access: 6,
    }));
    pend::revoke(caller(), readonly).unwrap();
    assert_eq!(slot_status(slot).obligations, 0);
    rejected(try_call(Action::Publish {
        memo: second,
        expected: memo_status(second).generation,
        maintenance: None,
    }));

    let private = share_from(slot, true, Distributable::No);
    let (local, _) = reserve(space());
    mutate(associate(local, share_source(private)));
    publish(local);
    rejected(try_call(associate(
        writable,
        Source::Memo {
            position: local,
            expected: memo_status(local).generation,
        },
    )));
    let bytes = AccordRequest {
        src: private,
        target: AccordTarget::Caller,
        limits: pend_info(private).limits,
        mark: Mark::NONE,
    }
    .to_bytes();
    assert!(pend::accord(VirtAddr::new(bytes.as_ptr() as usize)).is_err());
    retract(local);

    // A target branch can retire only its own publication; the source stays live.
    let child = derive(local.token(), pend_info(local.token()).limits);
    let target = at(local, child, 0);
    publish(target);
    pend::revoke(caller(), child).unwrap();
    assert_eq!(slot_status(slot).obligations, 0);
    publish(local);
    retract(local);
    race(slot);
    let _ = writeln!(
        Console,
        "PASS acceptance-lineage flat-share-memo=1 readonly=preserved nondistributable=preserved source-revoke=retired target-revoke=local narrow-insert-publish-race=closed"
    );
}
fn derivation_controls() {
    let slot = create(3);
    let limits = Limits {
        operations: Operations::Share(ShareRights::ASSOCIATE | ShareRights::DELETE),
        ..pend_info(slot.token()).limits
    };
    let request = |src, limits| {
        AccordRequest {
            src,
            target: AccordTarget::Caller,
            limits,
            mark: Mark::NONE,
        }
        .to_bytes()
    };
    let bytes = request(slot.token(), limits);
    assert!(share_view::derive(PendId::NONE, VirtAddr::new(bytes.as_ptr() as usize)).is_err());
    let token = share_view::derive(license(), VirtAddr::new(bytes.as_ptr() as usize)).unwrap();
    let partial = derive(
        token,
        Limits {
            scope: PendScope::Pages(PendRange::pages(1, 1).unwrap()),
            ..pend_info(token).limits
        },
    );
    assert!(share_view::delete(partial).is_err());
    assert_eq!(slot_status(slot).phase, SlotPhase::Fresh);
    let source = derive(
        slot.token(),
        Limits {
            operations: Operations::Flat(FlatRights::INSPECT | FlatRights::ASSOCIATE),
            ..pend_info(slot.token()).limits
        },
    );
    let bytes = request(
        source,
        Limits {
            operations: Operations::Share(ShareRights::ASSOCIATE),
            ..limits
        },
    );
    assert!(share_view::derive(license(), VirtAddr::new(bytes.as_ptr() as usize)).is_err());
    let source = derive(
        slot.token(),
        Limits {
            modifiable: Modifiable::No,
            ..pend_info(slot.token()).limits
        },
    );
    let bytes = request(source, limits);
    assert!(share_view::derive(license(), VirtAddr::new(bytes.as_ptr() as usize)).is_err());
    share_view::delete(token).unwrap();
    let _ = writeln!(
        Console,
        "PASS acceptance-share license=required source-share=required write-upgrade=reject partial-delete=reject"
    );
}
fn race(slot: SlotPosition) {
    let worker = Worker::new();
    for inserting in [false, true, false, true] {
        let source = derive(slot.token(), pend_info(slot.token()).limits);
        let position = SlotPosition::new(source, slot.flat(), 0).unwrap();
        let (memo, _) = reserve(space());
        if !inserting {
            insert(memo, position);
        }
        let token = accord(memo.token(), worker.0, pend_info(memo.token()).limits);
        let source_token = inserting.then(|| accord(source, worker.0, pend_info(source).limits));
        worker.begin(if inserting {
            Action::Insert {
                target: at(memo, token, 0),
                expected: memo_status(memo).generation,
                source: Source::Flat {
                    position: SlotPosition::new(source_token.unwrap(), slot.flat(), 0).unwrap(),
                    expected: slot_status(slot).version,
                },
            }
        } else {
            Action::Publish {
                memo: at(memo, token, 0),
                expected: memo_status(memo).generation,
                maintenance: None,
            }
        });
        let limits = Limits {
            operations: Operations::Flat(FlatRights::INSPECT),
            ..pend_info(source).limits
        };
        if inserting {
            let bytes = NarrowRequest {
                token: source,
                limits,
            }
            .to_bytes();
            assert_eq!(
                pend::narrow(VirtAddr::new(bytes.as_ptr() as usize))
                    .unwrap_err()
                    .into_source(),
                kernel_api::abi::call::pend::PendFail::Busy
            );
        } else {
            narrow(source, limits);
        }
        match worker.finish() {
            Ok(Response::Reply(r)) => assert!(matches!(
                r.outcome(),
                Outcome::Committed(_)
                    | Outcome::Rejected(
                        Failure::Denied | Failure::Dead | Failure::Stale | Failure::Busy
                    )
            )),
            other => panic!("race: {other:?}"),
        }
        if let Some(child) = source_token {
            pend::revoke(worker.0, child).unwrap();
            narrow(source, limits);
        }
        assert_eq!(slot_status(slot).obligations, 0);
        rejected(try_call(Action::Publish {
            memo,
            expected: memo_status(memo).generation,
            maintenance: None,
        }));
    }
    worker.stop();
}
