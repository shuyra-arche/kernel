use super::*;
use crate::{fs, machine};

fn reset() {
    assert_eq!(machine::sync::BUSY.load(Ordering::Relaxed), 0);
    *HOST.try_lock().unwrap() = None;
    FAILURE.store(0, Ordering::Release);
    machine::clock::TICKS.store(0, Ordering::Relaxed);
    fs::CREATE_FAIL.store(false, Ordering::Relaxed);
    fs::CREATE_CALLS.store(0, Ordering::Relaxed);
    fs::WRITE_FAIL_AT.store(usize::MAX, Ordering::Relaxed);
    fs::WRITE_CALLS.store(0, Ordering::Relaxed);
    fs::BYTES.lock().unwrap().clear();
}
fn observed_failure() -> Option<Failure> {
    Failure::decode(FAILURE.load(Ordering::Acquire))
}

#[test]
fn crash_report_is_complete_or_latches_the_first_delivery_failure() {
    reset();
    let mut report = Report::default();
    report
        .paragraph("exception", Some("fault detail".into()))
        .items
        .push(vec![Some("reason".into()), Some("invalid address".into())]);
    export(report.seal());
    assert!(complete());
    let bytes = fs::BYTES.lock().unwrap().clone();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["paras"][0]["name"], "exception");
    assert_eq!(value["paras"][0]["title"], "fault detail");
    assert!(bytes.ends_with(b"\n"));
    assert_eq!(EXPORT_NAME.to_bytes(), b"kernel-crash.json");

    reset();
    fs::WRITE_FAIL_AT.store(2, Ordering::Relaxed);
    export(&Report::default());
    assert!(!complete(), "a failed newline must reject a partial report");
    assert_eq!(observed_failure(), Some(Failure::Write));
    let writes = fs::WRITE_CALLS.load(Ordering::Relaxed);
    fail(Failure::Encode);
    export(&Report::default());
    assert_eq!(observed_failure(), Some(Failure::Write));
    assert_eq!(fs::WRITE_CALLS.load(Ordering::Relaxed), writes);

    reset();
    fs::CREATE_FAIL.store(true, Ordering::Relaxed);
    export(&Report::default());
    assert_eq!(observed_failure(), Some(Failure::Create));
    fs::CREATE_FAIL.store(false, Ordering::Relaxed);
    export(&Report::default());
    assert_eq!(fs::CREATE_CALLS.load(Ordering::Relaxed), 1);
    assert!(fs::BYTES.lock().unwrap().is_empty());

    reset();
    {
        let _stopped_owner = HOST.try_lock().unwrap();
        machine::clock::TICKS.store(u64::MAX - 5000, Ordering::Relaxed);
        export(&Report::default());
        assert_eq!(observed_failure(), Some(Failure::LockTimeout));
        assert!(
            !complete(),
            "a crash report cannot wait for a stopped lock owner"
        );
    }
    assert!(fs::BYTES.lock().unwrap().is_empty());
}
