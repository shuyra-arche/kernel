use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use kernel_api::model::unit::ExitScope;
use kernel_api::{
    abi::{
        call::{pend::PendFail, sign::SignFail},
        reply::SignReply,
    },
    address::VirtAddr,
    model::{
        identity::{Mark, PendId, TaskId},
        modifiable::Modifiable,
        pend::{
            AccordRequest, AccordTarget, Distributable, Limits, NarrowRequest, Operations,
            PendScope,
        },
        sign::{SignRights, SignSource},
        unit::UnitTarget,
        wait::Wait,
    },
};
use kernel_client::call::{pend, room, sign, unit};
static TOKEN: AtomicUsize = AtomicUsize::new(0);
static ENTERED: AtomicUsize = AtomicUsize::new(0);
static REVOKE_FINISHED: AtomicBool = AtomicBool::new(false);
extern "C" fn recipient() -> ! {
    let token = PendId::mint(TOKEN.load(Ordering::Acquire));
    ENTERED.store(1, Ordering::Release);
    assert!(matches!(
        sign::listen(token, Wait::Infinite).unwrap_err().source,
        SignFail::Denied | SignFail::Dead
    ));
    // Logical invalidation can wake us before physical ledger cleanup ends.
    // Keep the original registration alive for the controller's retries.
    while !REVOKE_FINISHED.load(Ordering::Acquire) {
        room::yield_();
    }
    unit::exit(37, 0, VirtAddr::new(0), 0, ExitScope::Task)
}
fn accord(
    src: PendId,
    task: TaskId,
    rights: SignRights,
) -> kernel_api::abi::call::pend::PendResult<PendId> {
    let bytes = AccordRequest {
        src,
        target: AccordTarget::Task(task),
        mark: Mark::NONE,
        limits: Limits {
            operations: Operations::Sign(rights),
            modifiable: Modifiable::No,
            distributable: Distributable::Yes,
            scope: PendScope::Object,
        },
    }
    .to_bytes();
    pend::accord(super::ptr(&bytes))
}

pub(super) fn run() {
    let root = sign::create(crate::license::get()).unwrap();
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
    let observed = super::pend::inspect(root);
    assert!(matches!(
        observed.target,
        kernel_api::model::pend::Target::Sign(_)
    ));
    let listener = accord(root, super::lifecycle::self_id(), SignRights::LISTEN).unwrap();
    assert_eq!(super::pend::inspect(listener).target, observed.target);
    assert_eq!(
        sign::trigger(listener).unwrap_err().source,
        SignFail::Denied
    );
    assert_eq!(
        sign::consume(listener, second).unwrap_err().source,
        SignFail::Denied
    );
    assert_eq!(sign::delete(listener).unwrap_err().source, SignFail::Denied);
    pend::return_(listener).unwrap();

    let source = sign::create(crate::license::get()).unwrap();
    let routed = accord(
        root,
        super::lifecycle::self_id(),
        SignRights::LISTEN | SignRights::ROUTE,
    )
    .unwrap();
    let source_event = SignSource::Sign { token: source };
    sign::attach(routed, source_event, 0).unwrap();
    sign::trigger(source).unwrap();
    assert_eq!(
        sign::listen(root, Wait::Poll).unwrap(),
        SignReply::Source {
            source: source_event,
            fail: None
        }
    );
    let bytes = NarrowRequest {
        token: routed,
        limits: Limits {
            operations: Operations::Sign(SignRights::LISTEN),
            distributable: Distributable::No,
            ..super::pend::inspect(routed).limits
        },
    }
    .to_bytes();
    pend::narrow(super::ptr(&bytes)).unwrap();
    assert_eq!(sign::listen(root, Wait::Poll).unwrap(), SignReply::Pending);
    assert!(accord(routed, super::lifecycle::self_id(), SignRights::LISTEN).is_err());
    pend::return_(routed).unwrap();
    sign::delete(source).unwrap();
    pend::return_(source).unwrap();

    let task = super::lifecycle::spawn(recipient as *const () as usize);
    let branch = accord(root, task, SignRights::LISTEN).unwrap();
    TOKEN.store(branch.get(), Ordering::Release);
    unit::resume(UnitTarget::Task(task)).unwrap();
    while ENTERED.load(Ordering::Acquire) == 0 {
        room::yield_();
    }
    // Exercise revocation after the receiver has had a chance to enqueue.
    room::sleep_until(room::clock().saturating_add(2_000_000)).unwrap();
    let deadline = room::clock().saturating_add(2_000_000_000);
    loop {
        match pend::revoke(task, branch) {
            Ok(()) => break,
            Err(error) if error.source == PendFail::Busy => {
                assert!(room::clock() < deadline, "Sign revocation did not settle");
                room::yield_();
            }
            Err(error) => panic!("Sign revocation failed: {error:?}"),
        }
    }
    REVOKE_FINISHED.store(true, Ordering::Release);
    assert!(super::lifecycle::wait_task(task));
    assert_eq!(super::lifecycle::result(task), (1, 37));
    super::lifecycle::delete(task);
    // Returning an intermediate handle invalidates its descendants even in this Task.
    let child = accord(root, super::lifecycle::self_id(), SignRights::all()).unwrap();
    let leaf = accord(child, super::lifecycle::self_id(), SignRights::LISTEN).unwrap();
    pend::return_(child).unwrap();
    // Reusing the intermediate registration slot cannot revive its descendants.
    let replacement = accord(root, super::lifecycle::self_id(), SignRights::all()).unwrap();
    assert_ne!(replacement, child);
    assert_eq!(
        sign::listen(child, Wait::Poll).unwrap_err().source,
        SignFail::Denied
    );
    sign::trigger(replacement).unwrap();
    assert_eq!(
        sign::listen(leaf, Wait::Poll).unwrap_err().source,
        SignFail::Denied
    );
    assert!(pend::return_(leaf).is_err());
    pend::return_(replacement).unwrap();
    sign::delete(root).unwrap();
    pend::return_(root).unwrap();
}
