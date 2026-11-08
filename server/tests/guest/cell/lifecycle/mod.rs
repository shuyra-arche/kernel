use kernel_api::{
    abi::reply::SignReply,
    address::VirtAddr,
    model::{
        identity::TaskId,
        sign::SignSource,
        unit::{UnitCreate, UnitInfo, UnitQuery, UnitState, UnitTarget},
        wait::Wait,
    },
};
use kernel_client::call::{pend, room, sign, unit};
pub(super) fn spawn(entry: usize) -> TaskId {
    let bytes = UnitCreate::Task {
        team: None,
        entry: VirtAddr::new(entry),
        args: VirtAddr::new(0),
        count: 0,
        stack: VirtAddr::new(0),
    }
    .to_bytes();
    let UnitTarget::Task(id) = unit::create(VirtAddr::new(bytes.as_ptr() as usize)).unwrap() else {
        panic!("created wrong kind")
    };
    id
}
pub(super) fn inspect(query: UnitQuery) -> UnitInfo {
    let mut bytes = [0; UnitInfo::WIDTH];
    assert!(unit::inspect(query, VirtAddr::new(bytes.as_mut_ptr() as usize)).unwrap());
    UnitInfo::from_bytes(&bytes).unwrap()
}
pub(super) fn self_id() -> TaskId {
    let UnitTarget::Task(id) = inspect(UnitQuery::Caller).target else {
        panic!("caller is a Task")
    };
    id
}
pub(super) fn wait_task(id: TaskId) -> bool {
    let token = sign::create(crate::license::get()).unwrap();
    let source = SignSource::Join {
        target: UnitTarget::Task(id),
    };
    sign::attach(token, source).unwrap();
    let reply = sign::listen(token, Wait::At(room::clock().saturating_add(5_000_000_000))).unwrap();
    assert!(matches!(reply, SignReply::Source { source: found, fail: None } if found == source));
    sign::detach(token, source).unwrap();
    sign::delete(token).unwrap();
    pend::return_(token).unwrap();
    inspect(UnitQuery::Target(UnitTarget::Task(id))).state == UnitState::Exited
}
pub(super) fn result(id: TaskId) -> (usize, usize) {
    let query = UnitQuery::Target(UnitTarget::Task(id));
    let first = inspect(query);
    let second = inspect(query);
    assert_eq!(first.result, second.result);
    first.result.map_or((0, 0), |value| (1, value as usize))
}
pub(super) fn delete(id: TaskId) {
    let target = UnitTarget::Task(id);
    let deadline = room::clock().saturating_add(5_000_000_000);
    while !inspect(UnitQuery::Target(target)).reclaimed {
        assert!(room::clock() < deadline);
        room::yield_();
    }
    unit::delete(target).unwrap();
    let mut bytes = [0; UnitInfo::WIDTH];
    assert!(
        unit::inspect(
            UnitQuery::Target(target),
            VirtAddr::new(bytes.as_mut_ptr() as usize)
        )
        .is_err()
    );
}
