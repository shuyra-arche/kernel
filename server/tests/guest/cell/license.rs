use core::sync::atomic::{AtomicUsize, Ordering};
use kernel_api::{
    abi::call::{clone_view::CloneFail, share_view::ShareFail, sign::SignFail},
    model::{
        identity::{Mark, PendId, TaskId},
        modifiable::Modifiable,
        pend::{
            AccordRequest, AccordTarget, CreationRights, Distributable, Limits, NarrowRequest,
            Operations, PendInfo, PendRange, Target,
        },
    },
};
use kernel_client::call::{clone_view, pend, share_view, sign};
static FOREIGN: AtomicUsize = AtomicUsize::new(0);
extern "C" fn recipient() -> ! {
    let foreign = PendId::mint(FOREIGN.load(Ordering::Acquire));
    assert_eq!(sign::create(foreign).unwrap_err().source, SignFail::Denied);
    let endpoint = sign::create(get()).unwrap();
    sign::delete(endpoint).unwrap();
    pend::return_(endpoint).unwrap();
    kernel_client::call::unit::exit(39, 0, kernel_api::address::VirtAddr::new(0), 0)
}

pub(super) fn get() -> PendId {
    let mut after = PendId::NONE;
    loop {
        let mut words = [0; PendInfo::WORDS];
        let count = pend::collect(
            after,
            kernel_api::address::VirtAddr::new(words.as_mut_ptr() as usize),
            1,
        )
        .unwrap();
        assert_eq!(count, 1, "no explicit creation license in this Task");
        let info = PendInfo::from_words(words).unwrap();
        if info.target == Target::None {
            return info.token;
        }
        after = info.token;
    }
}
pub(super) fn delegate(task: TaskId) {
    let bytes = AccordRequest {
        src: get(),
        target: AccordTarget::Task(task),
        mark: Mark::NONE,
        limits: Limits {
            operations: Operations::None(CreationRights::all()),
            modifiable: Modifiable::No,
            distributable: Distributable::Yes,
            range: PendRange::Inherit,
        },
    }
    .to_bytes();
    pend::accord(super::ptr(&bytes)).unwrap();
}
pub(super) fn run() {
    let root = get();
    FOREIGN.store(root.get(), Ordering::Release);
    let task = super::lifecycle::spawn(recipient as *const () as usize);
    delegate(task);
    kernel_client::call::unit::resume(kernel_api::model::unit::UnitTarget::Task(task)).unwrap();
    assert!(super::lifecycle::wait_task(task));
    assert_eq!(super::lifecycle::result(task), (1, 39));
    super::lifecycle::delete(task);
    let bytes = AccordRequest {
        src: root,
        target: AccordTarget::Caller,
        mark: Mark::NONE,
        limits: Limits {
            operations: Operations::None(CreationRights::SIGN),
            modifiable: Modifiable::No,
            distributable: Distributable::No,
            range: PendRange::Inherit,
        },
    }
    .to_bytes();
    let license = pend::accord(super::ptr(&bytes)).unwrap();
    assert_eq!(
        sign::create(PendId::NONE).unwrap_err().source,
        SignFail::Denied
    );
    assert_eq!(
        clone_view::create(license, Mark::NONE, Default::default())
            .unwrap_err()
            .source,
        CloneFail::Denied
    );
    let slot = super::allocate(1);
    assert_eq!(
        share_view::create(license, slot, Modifiable::Yes)
            .unwrap_err()
            .source,
        ShareFail::Denied
    );
    let endpoint = sign::create(license).unwrap();
    assert_eq!(sign::create(endpoint).unwrap_err().source, SignFail::Denied);
    let bytes = NarrowRequest {
        token: license,
        limits: Limits {
            operations: Operations::None(CreationRights::empty()),
            ..super::pend::inspect(license).limits
        },
    }
    .to_bytes();
    pend::narrow(super::ptr(&bytes)).unwrap();
    assert_eq!(sign::create(license).unwrap_err().source, SignFail::Denied);
    pend::return_(license).unwrap();
    // Revoking the right to create future objects does not destroy created ones.
    sign::trigger(endpoint).unwrap();
    sign::delete(endpoint).unwrap();
    pend::return_(endpoint).unwrap();
    super::delete(kernel_api::model::cell::CellObject::Slot(slot));
}
