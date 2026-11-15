#![allow(dead_code)]
#[path = "../utilities.rs"]
mod utilities;
#[path = "../verify.rs"]
mod verify;
#[test]
fn comparison_control_rejects_missing_duplicate_and_malformed_inputs() {
    let pin = "0123456789abcdef0123456789abcdef01234567";
    assert_eq!(
        utilities::control(&format!("Room baseline: {pin}\n")).unwrap(),
        pin
    );
    for body in [
        "".into(),
        format!("Room baseline: {pin}\nRoom baseline: {pin}"),
        format!("Room baseline: {pin}\nRoom baseline: bad"),
        "Room baseline: $(id)".into(),
        format!("Room baseline: {pin} "),
        format!("Room baseline: {}", pin.to_uppercase()),
    ] {
        assert!(utilities::control(&body).is_err());
    }
}
#[test]
fn smp_requires_each_actual_writer_to_migrate() {
    let log = "task: all tasks exited\nPASS native-memory\nPASS native-fault\nPASS acceptance-unit\nPASS acceptance-scopes\nPASS acceptance-lineage\nPASS acceptance-maintenance\nPASS smp-completion\nPASS smp-instructions\nPASS smp-retirement\nPASS smp-executable-rejected\nPASS smp-writer task=1\nPASS smp-writer task=2\n";
    assert_eq!(
        verify::check(log, &verify::Kind::Smp, 1).unwrap()["passed"],
        true
    );
    assert_eq!(
        verify::check(log, &verify::Kind::Smp, 4).unwrap()["passed"],
        false
    );
    assert_eq!(
        verify::check(
            &format!("{log}TRACE smp-migration task=1\nTRACE smp-migration task=2"),
            &verify::Kind::Smp,
            4
        )
        .unwrap()["passed"],
        true
    );
}
#[test]
fn activity_rejection_requires_zero_hardware_capture_and_bounded_work() {
    let log = "PASS activity unsupported=1\nPASS activity-fences harts=4\ntask: all tasks exited\n";
    let kind = verify::Kind::Activity {
        unsupported: true,
        stopped: false,
    };
    assert_eq!(verify::check(log, &kind, 4).unwrap()["passed"], false);
    for row in [
        "max_steps=9 captures=0",
        "max_steps=0 captures=1",
        "captures=0",
    ] {
        assert_eq!(
            verify::check(&format!("{log}TRACE activity-budget {row}\n"), &kind, 4).unwrap()["passed"],
            false
        );
    }
    assert_eq!(
        verify::check(
            &format!("{log}TRACE activity-budget max_steps=0 captures=0\n"),
            &kind,
            4
        )
        .unwrap()["passed"],
        true
    );
}
#[test]
fn flat_requires_all_contracts_and_rejects_guest_failure() {
    let kind = verify::Kind::Flat { timing: false };
    let log = "PASS flat-contracts\nPASS flat-oom\nPASS flat-reservation\nPASS flat-cancel\nPASS flat-race\ntask: all tasks exited";
    assert_eq!(verify::check(log, &kind, 4).unwrap()["passed"], true);
    assert_eq!(
        verify::check(&format!("{log}\nFAIL"), &kind, 4).unwrap()["passed"],
        false
    );
    assert_eq!(
        verify::check(&log.replace("PASS flat-race", ""), &kind, 4).unwrap()["passed"],
        false
    );
}
