use super::*;
use kernel_client::call::{room, sign};

/// More than one public return batch. Unvisited authorities must already reject
/// use while the root/leaf records remain available for exact-identity retries.
pub(super) fn run() {
    let root = sign::create(crate::license::get()).unwrap();
    let branch = accord(
        root,
        AccordTarget::Caller,
        Modifiable::No,
        PendRange::Inherit,
    )
    .unwrap();
    let sibling = accord(
        root,
        AccordTarget::Caller,
        Modifiable::No,
        PendRange::Inherit,
    )
    .unwrap();
    let mut last = PendId::NONE;
    for _ in 0..80 {
        last = accord(
            branch,
            AccordTarget::Caller,
            Modifiable::No,
            PendRange::Inherit,
        )
        .unwrap();
    }
    assert_eq!(pend::return_(branch).unwrap_err().source, PendFail::Busy);
    assert_eq!(inspect(last).status, PendStatus::Retiring);
    assert!(sign::listen(last, kernel_api::model::wait::Wait::Poll).is_err());
    assert_eq!(inspect(sibling).status, PendStatus::Active);
    assert!(sign::listen(sibling, kernel_api::model::wait::Wait::Poll).is_ok());
    let deadline = room::clock().saturating_add(5_000_000_000);
    loop {
        match pend::return_(branch) {
            Ok(()) => break,
            Err(error) => {
                assert_eq!(error.source, PendFail::Busy);
                assert!(room::clock() < deadline, "Pend retirement stalled");
                room::yield_();
            }
        }
    }
    let mut words = [0; PendInfo::WORDS];
    assert!(pend::inspect(last, VirtAddr::new(words.as_mut_ptr() as usize)).is_err());
    assert!(sign::listen(sibling, kernel_api::model::wait::Wait::Poll).is_ok());
    pend::return_(sibling).unwrap();
    sign::delete(root).unwrap();
    pend::return_(root).unwrap();
    use core::fmt::Write;
    writeln!(
        crate::Console,
        "PASS pend-retirement bounded=1 closed=1 sibling=1 retry=1"
    )
    .unwrap();
}
