#[path = "../../src/basis/pend/source/creation.rs"]
mod qualification;
use kernel_api::abi::call::pend::PendFail;
use qualification::Creation;

#[test]
fn failed_adoption_returns_qualification_but_success_is_one_use() {
    let source = Creation::new();
    let claim = source.claim().unwrap();
    assert!(source.claimed());
    assert!(matches!(source.claim(), Err(PendFail::Busy)));
    drop(claim); // Any early return, including allocation/withdrawal failure.
    assert!(!source.claimed());
    source.claim().unwrap().commit();
    assert!(matches!(source.claim(), Err(PendFail::Denied)));
}
#[test]
fn final_hold_expiry_cannot_be_undone_by_claim_rollback() {
    let source = Creation::new();
    let claim = source.claim().unwrap();
    source.expire();
    drop(claim);
    assert!(matches!(source.claim(), Err(PendFail::Denied)));
}
#[test]
fn competing_creators_never_own_the_same_qualification() {
    let source = std::sync::Arc::new(Creation::new());
    let claim = source.claim().unwrap();
    let other = source.clone();
    std::thread::spawn(move || assert!(matches!(other.claim(), Err(PendFail::Busy))))
        .join()
        .unwrap();
    claim.commit();
    assert!(matches!(source.claim(), Err(PendFail::Denied)));
}
