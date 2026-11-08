use core::sync::atomic::{AtomicUsize, Ordering};
use kernel_api::{
    abi::{call::sign::SignFail, reply::SignReply},
    address::VirtAddr,
    model::{
        sign::{SignId, SignRights},
        unit::UnitTarget,
        wait::Wait,
    },
};
use kernel_client::call::{room, sign, unit};
static TOKEN: AtomicUsize = AtomicUsize::new(0);
static ENTERED: AtomicUsize = AtomicUsize::new(0);
extern "C" fn recipient() -> ! {
    let token = SignId::new(TOKEN.load(Ordering::Acquire));
    ENTERED.store(1, Ordering::Release);
    assert_eq!(
        sign::listen(token, Wait::Infinite).unwrap_err().source,
        SignFail::Dead
    );
    sign::return_(token).unwrap();
    unit::exit(37, 0, VirtAddr::new(0), 0)
}
pub(super) fn run() {
    let root = sign::create().unwrap();
    assert_eq!(sign::listen(root, Wait::Poll).unwrap(), SignReply::Pending);
    sign::trigger(root).unwrap();
    let SignReply::Triggered { sequence: first } = sign::listen(root, Wait::Poll).unwrap() else {
        panic!("missing event")
    };
    assert_eq!(
        sign::listen(root, Wait::Poll).unwrap(),
        SignReply::Triggered { sequence: first }
    );
    assert_eq!(sign::trigger(root).unwrap_err().source, SignFail::Busy);
    sign::consume(root, first).unwrap();
    sign::trigger(root).unwrap();
    let SignReply::Triggered { sequence: second } = sign::listen(root, Wait::Poll).unwrap() else {
        panic!("missing event")
    };
    assert!(second > first);
    assert_eq!(
        sign::consume(root, first).unwrap_err().source,
        SignFail::Busy
    );
    sign::consume(root, second).unwrap();
    let until = room::clock().saturating_add(2_000_000);
    assert_eq!(
        sign::listen(root, Wait::At(until)).unwrap(),
        SignReply::Pending
    );
    assert!(room::clock() >= until);
    let until = room::clock().saturating_add(2_000_000);
    room::sleep_until(until).unwrap();
    assert!(room::clock() >= until);
    room::sleep_until(0).unwrap();
    let task = super::lifecycle::spawn(recipient as *const () as usize);
    let branch = sign::accord(root, task, SignRights::LISTEN).unwrap();
    TOKEN.store(branch.get(), Ordering::Release);
    unit::resume(UnitTarget::Task(task)).unwrap();
    while ENTERED.load(Ordering::Acquire) == 0 {
        room::yield_();
    }
    // Exercise revocation after the receiver has had a chance to enqueue.
    room::sleep_until(room::clock().saturating_add(2_000_000)).unwrap();
    sign::revoke(root, task, branch).unwrap();
    assert!(super::lifecycle::wait_task(task));
    assert_eq!(super::lifecycle::result(task), (1, 37));
    super::lifecycle::delete(task);
    // Returning an intermediate handle invalidates its descendants even in this Task.
    let child = sign::accord(root, super::lifecycle::self_id(), SignRights::all()).unwrap();
    let leaf = sign::accord(child, super::lifecycle::self_id(), SignRights::LISTEN).unwrap();
    sign::return_(child).unwrap();
    // Reusing the intermediate registration slot cannot revive its descendants.
    let replacement = sign::accord(root, super::lifecycle::self_id(), SignRights::all()).unwrap();
    assert_ne!(replacement, child);
    assert_eq!(
        sign::listen(child, Wait::Poll).unwrap_err().source,
        SignFail::Denied
    );
    sign::trigger(replacement).unwrap();
    assert_eq!(
        sign::listen(leaf, Wait::Poll).unwrap_err().source,
        SignFail::Dead
    );
    sign::return_(leaf).unwrap();
    sign::return_(replacement).unwrap();
    sign::delete(root).unwrap();
    sign::return_(root).unwrap();
}
