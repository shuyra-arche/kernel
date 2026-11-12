//! The actual TeamNode and Member implementation runs against host lock and
//! notification boundaries. Lock counters detect unrelated subtree traversal.
use crate::{
    basis::unit::anchor::Anchor,
    lifecycle::{TeamNode, TeamStage, TeamState, exit},
    membership::{Member, Receipt},
    metal::sync::SpinLock,
    registration::{TaskState, TaskWeak},
};
use kernel_api::model::identity::{TaskId, TeamId};
use std::sync::{Arc, atomic::AtomicBool};

pub(crate) fn node(id: usize, parent: Option<&Arc<TeamNode>>) -> Arc<TeamNode> {
    let node = Arc::new(TeamNode {
        id: TeamId::new(id),
        owner: TaskId::new(1),
        parent: parent.cloned(),
        state: SpinLock::new(TeamState::new(TeamStage::Ready { default_entry: 0 })),
        tasks: SpinLock::new(Vec::new()),
        children: SpinLock::new(Vec::new()),
        life: Arc::new(Anchor),
        exit: exit::ExitPolicy::new(),
        member_exit: false,
        termination: SpinLock::new(crate::basis::unit::lifecycle::Termination::new()),
    });
    if let Some(parent) = parent {
        parent.children.lock().push(node.clone());
    }
    node
}
pub(crate) fn member(node: &Arc<TeamNode>, id: usize, owners: &[usize]) -> Arc<Member> {
    let member = Arc::new(Member {
        id: TaskId::new(id),
        team: node.id,
        controller: TaskId::new(1),
        creator: TaskId::new(1),
        sire: TaskId::new(1),
        reclaimed: AtomicBool::new(false),
        task: TaskWeak::empty(),
        node: Arc::downgrade(node),
        state: Arc::new(SpinLock::new(TaskState)),
        life: Arc::new(Anchor),
        result: SpinLock::new(Some(83)),
        receipts: SpinLock::new(
            owners
                .iter()
                .map(|owner| Receipt {
                    owner: TaskId::new(*owner),
                    roots: vec![node.id],
                })
                .collect(),
        ),
    });
    node.tasks.lock().push(member.clone());
    member
}

#[test]
fn unobserved_members_remain_until_reclaimed_then_only_empty_ancestors_detach() {
    let root = node(1, None);
    let parent = node(2, Some(&root));
    let leaf = node(3, Some(&parent));
    let sibling = node(4, Some(&root));
    parent.state.lock().discard();
    leaf.state.lock().discard();
    let task = member(&leaf, 9, &[1]);
    let untouched = sibling.tasks.locks();
    assert!(task.receive(TaskId::new(1)));
    assert!(leaf.member(task.id).is_some());
    assert_eq!(root.children.lock().len(), 2);
    task.reclaim();
    assert!(leaf.tasks.lock().is_empty());
    assert!(parent.children.lock().is_empty());
    assert_eq!(root.children.lock().len(), 1);
    assert_eq!(root.children.lock()[0].id, sibling.id);
    assert_eq!(sibling.tasks.locks(), untouched);
    // Repeated notifications do not remove a different child at the old index.
    task.reclaim();
    leaf.prune();
    assert_eq!(root.children.lock()[0].id, sibling.id);
}

#[test]
fn reclaimed_members_keep_each_observers_exit_result_until_its_receipt_is_removed() {
    let root = node(1, None);
    let child = node(2, Some(&root));
    child.state.lock().discard();
    let task = member(&child, 9, &[1, 2]);
    task.reclaim();
    assert!(task.receive(TaskId::new(1)));
    assert!(!task.receive(TaskId::new(1)));
    assert!(child.member(task.id).is_some());
    assert_eq!(*task.result.lock(), Some(83));
    assert!(task.receive(TaskId::new(2)));
    assert!(child.tasks.lock().is_empty());
    assert!(root.children.lock().is_empty());
    assert_eq!(*task.result.lock(), Some(83));
}

#[test]
fn owner_clear_does_not_skip_siblings_removed_during_traversal() {
    let root = node(1, None);
    for id in 2..8 {
        let child = node(id, Some(&root));
        child.state.lock().discard();
        member(&child, id, &[1]).reclaim();
    }
    let live = node(8, Some(&root));
    let task = member(&live, 9, &[2]);
    task.reclaim();
    root.clear(TaskId::new(1), None);
    assert_eq!(root.children.lock().len(), 1);
    assert_eq!(root.children.lock()[0].id, live.id);
    assert!(task.pending(TaskId::new(2)));
    // A live Team is not detached even after its last result is released.
    assert!(task.receive(TaskId::new(2)));
    assert_eq!(root.children.lock().len(), 1);
}

#[test]
fn clearing_one_root_preserves_other_receipt_roots_and_unreclaimed_members() {
    let root = node(1, None);
    let child = node(2, Some(&root));
    child.state.lock().discard();
    let task = member(&child, 9, &[1]);
    task.receipts.lock()[0].roots.push(root.id);
    task.reclaim();
    child.clear(TaskId::new(1), Some(child.id));
    assert!(child.member(task.id).is_some());
    assert_eq!(task.receipts.lock()[0].roots, vec![root.id]);
    child.clear(TaskId::new(1), Some(root.id));
    assert!(root.children.lock().is_empty());
}

#[test]
fn receipt_clearance_advances_one_member_and_keeps_other_owners_results() {
    let root = node(1, None);
    let child = node(2, Some(&root));
    let sibling = node(3, Some(&root));
    child.state.lock().discard();
    let first = member(&child, 4, &[1]);
    let second = member(&child, 5, &[1]);
    let retained = member(&sibling, 6, &[1, 7]);
    first.reclaim();
    second.reclaim();
    retained.reclaim();
    let mut cursor = crate::lifecycle::Clearance::new(root.clone());
    let untouched = sibling.tasks.locks();
    assert!(!cursor.advance(TaskId::new(1), None)); // root members
    assert!(!cursor.advance(TaskId::new(1), None)); // enter child
    assert!(!cursor.advance(TaskId::new(1), None)); // one member
    assert!(!first.pending(TaskId::new(1)));
    assert!(second.pending(TaskId::new(1)));
    assert_eq!(sibling.tasks.locks(), untouched);
    let mut done = false;
    for _ in 0..20 {
        if cursor.advance(TaskId::new(1), None) {
            done = true;
            break;
        }
    }
    assert!(done);
    assert!(!second.pending(TaskId::new(1)));
    assert!(!retained.pending(TaskId::new(1)));
    assert!(retained.pending(TaskId::new(7)));
    assert_eq!(*retained.result.lock(), Some(83));
    assert_eq!(root.children.lock().len(), 1);
}

#[test]
fn finishing_keeps_pause_independent_and_oust_never_reopens_admission() {
    use kernel_api::abi::call::unit::UnitFail;

    let mut state = TeamState::new(TeamStage::Constructing);
    assert!(state.admitting());
    assert!(!state.ready());
    assert_eq!(state.default_entry(), 0);
    assert_eq!(state.oust(), Err(UnitFail::Busy));
    state.set_default_entry(0x1000);
    state.debark().unwrap();
    state.finish();
    assert!(state.paused());
    assert!(state.ready());
    assert!(!state.admitting());
    state.embark().unwrap();
    assert!(!state.paused());
    assert!(!state.admitting());
    use kernel_api::model::unit::ExitCause;
    let first = super::lifecycle::TeamExit {
        cause: ExitCause::Reap,
        reason: 0,
        value: Some(23),
    };
    state.doom(Some(first));
    state.doom(None);
    assert_eq!(state.exit(), Some(first));
    assert_eq!(state.embark(), Err(UnitFail::Denied));
    assert_eq!(state.debark(), Err(UnitFail::Denied));
    assert_eq!(state.default_entry(), 0);
    state.oust().unwrap();
    state.doom(None);
    state.oust().unwrap();
    assert!(state.ousted());
    assert!(!state.admitting());
}
