//! Actual Accord/Ledger admission and cleanup; Task execution/IRQ are fixtures.
use super::{Window, allocation, branch, control, ledger, support::Task, target::Target};
use kernel_api::{
    abi::call::pend::PendFail,
    model::{
        identity::{Mark, PendId},
        modifiable::Modifiable,
        pend::{CreationRights, Distributable, Limits, Operations, PendRange},
    },
};
use std::sync::{Arc, atomic::Ordering};

fn root(task: &Arc<Task>) -> Arc<ledger::PendEntry> {
    let entry = Arc::new(
        ledger::PendEntry::try_new(
            Target::None {
                owner: task.ident.id,
            },
            Mark::NONE,
            Modifiable::No,
            None,
        )
        .unwrap(),
    );
    task.pend.prepare(entry.clone()).unwrap().publish();
    entry
}
fn grant(
    sender: &Arc<Task>,
    target: &Arc<Task>,
    root: &ledger::PendEntry,
) -> Result<PendId, PendFail> {
    branch::accord(
        sender,
        root.token(),
        &Arc::downgrade(target),
        Limits {
            operations: Operations::None(CreationRights::SIGN),
            distributable: Distributable::No,
            ..root.authority().limits()
        },
        Mark::NONE,
        Window::Parent(PendRange::Inherit),
        None,
    )
    .map(PendId::mint)
}

#[test]
fn unrelated_tasks_delegate_live_authority_without_acquiring_execution_control() {
    let sender = Task::in_team(1, 1);
    let receiver = Task::in_team(2, 2);
    assert!(!control::adjacent(&sender, &receiver));
    assert!(!control::manages(&sender, &receiver));
    let source = root(&sender);
    let token = grant(&sender, &receiver, &source).unwrap();
    let received = ledger::locate(&receiver, token).unwrap();
    assert!(received.permits(Operations::None(CreationRights::SIGN)));
    assert!(!received.permits(Operations::None(CreationRights::SHARE)));
    assert_eq!(ledger::vestor(&received), Some(sender.ident.id));
    assert!(!control::manages(&sender, &receiver));
    assert_eq!(grant(&receiver, &sender, &received), Err(PendFail::Denied));
    source.invalidate();
    assert!(!received.alive());
    assert_eq!(grant(&sender, &receiver, &source), Err(PendFail::Dead));
    ledger::return_(&receiver, token).unwrap();
    assert!(ledger::locate(&receiver, token).is_none());
    assert!(sender.pend.heirs.lock().is_empty());
}

#[test]
fn sender_budget_counts_all_parents_until_return_or_exit_and_failed_grants_leave_no_edge() {
    let sender = Task::in_team(1, 1);
    let receiver = Task::in_team(2, 2);
    let first = root(&sender);
    let second = root(&sender);
    let mut tokens: Vec<_> = (0..32)
        .map(|at| {
            grant(
                &sender,
                &receiver,
                if at % 2 == 0 { &first } else { &second },
            )
            .unwrap()
        })
        .collect();
    assert_eq!(
        grant(&sender, &receiver, &second),
        Err(PendFail::OutOfMemory)
    );
    assert_eq!(sender.pend.heirs.lock().len(), 32);
    assert_eq!(receiver.pend.entries.lock().len(), 32);
    ledger::return_(&receiver, tokens.pop().unwrap()).unwrap();
    tokens.push(grant(&sender, &receiver, &second).unwrap());
    receiver.dead.store(true, Ordering::Release);
    ledger::doom(&receiver);
    assert_eq!(grant(&sender, &receiver, &first), Err(PendFail::Dead));
    for _ in 0..64 {
        if ledger::retire(&receiver).is_ok() {
            break;
        }
    }
    assert_eq!(receiver.pend.entries.lock().len(), 0);
    assert!(sender.pend.heirs.lock().is_empty());
}

#[test]
fn team_budget_survives_sender_exit_and_another_team_retains_admission() {
    let receiver = Task::in_team(100, 2);
    let mut tokens = Vec::new();
    for id in 1..=4 {
        let sender = Task::in_team(id, 1);
        let source = root(&sender);
        tokens.extend((0..32).map(|_| grant(&sender, &receiver, &source).unwrap()));
        // Immutable billing identity survives the Weak<Task> becoming stale.
        drop(source);
        drop(sender);
    }
    let sender = Task::in_team(5, 1);
    let source = root(&sender);
    assert_eq!(
        grant(&sender, &receiver, &source),
        Err(PendFail::OutOfMemory)
    );
    let other = Task::in_team(6, 3);
    let other_source = root(&other);
    ledger::return_(&receiver, grant(&other, &receiver, &other_source).unwrap()).unwrap();
    ledger::return_(&receiver, tokens.pop().unwrap()).unwrap();
    let replacement = grant(&sender, &receiver, &source).unwrap();
    ledger::return_(&receiver, replacement).unwrap();
    for token in tokens {
        ledger::return_(&receiver, token).unwrap();
    }
}

#[test]
fn incoming_saturation_preserves_recipient_capacity_and_every_preparation_rolls_back() {
    let receiver = Task::in_team(1000, 1000);
    let mut senders = Vec::new();
    let mut tokens = Vec::new();
    for id in 1..=8 {
        let sender = Task::in_team(id, id);
        let source = root(&sender);
        tokens.extend((0..32).map(|_| grant(&sender, &receiver, &source).unwrap()));
        senders.push((sender, source));
    }
    let extra = Task::in_team(9, 9);
    let source = root(&extra);
    assert_eq!(
        grant(&extra, &receiver, &source),
        Err(PendFail::OutOfMemory)
    );
    for _ in 0..768 {
        root(&receiver);
    }
    assert_eq!(receiver.pend.entries.lock().len(), 1024);
    assert_eq!(
        receiver.pend.prepare(super::entry()).err(),
        Some(PendFail::OutOfMemory)
    );
    ledger::return_(&receiver, tokens.pop().unwrap()).unwrap();
    let token = grant(&extra, &receiver, &source).unwrap();
    ledger::return_(&receiver, token).unwrap();
    for token in tokens {
        ledger::return_(&receiver, token).unwrap();
    }

    let target = Task::in_team(2000, 2000);
    for fail in 0..16 {
        let before = extra.pend.heirs.lock().len();
        let result = allocation::fail_after(fail, || grant(&extra, &target, &source));
        match result {
            Ok(token) => ledger::return_(&target, token).unwrap(),
            Err(PendFail::OutOfMemory) => {}
            Err(error) => panic!("unexpected failure: {error:?}"),
        }
        assert_eq!(extra.pend.heirs.lock().len(), before);
        assert_eq!(target.pend.entries.lock().len(), 0);
    }
    for _ in 0..8 {
        let token = grant(&extra, &target, &source).unwrap();
        ledger::return_(&target, token).unwrap();
    }
}

#[test]
fn lender_identity_survives_source_ledger_contention_and_does_not_grant_authority() {
    let sender = Task::in_team(71, 7);
    let receiver = Task::in_team(81, 8);
    let source = root(&sender);
    let identity = sender.ident.id;
    let token = grant(&sender, &receiver, &source).unwrap();
    let received = ledger::locate(&receiver, token).unwrap();
    {
        // The source can be constructing/cleaning another object while its
        // live child is inspected. Contention cannot fabricate lender zero.
        let _source_transaction = sender.pend.lock();
        assert_eq!(ledger::vestor(&received), Some(identity));
    }
    source.invalidate();
    drop(source);
    drop(sender);
    assert_eq!(ledger::vestor(&received), Some(identity));
    assert!(!received.alive());
    assert!(!received.permits(Operations::None(CreationRights::SIGN)));
    ledger::return_(&receiver, token).unwrap();
    assert_eq!(ledger::vestor(&received), Some(identity));
    assert!(ledger::locate(&receiver, token).is_none());
}
