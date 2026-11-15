#![allow(dead_code)]
#[path = "../composition/mod.rs"]
mod composition;
#[path = "../utilities.rs"]
mod utilities;
#[path = "../verify.rs"]
mod verify;
#[cfg(unix)]
#[test]
fn symbolizer_keeps_literal_compressed_call_pcs_and_rejects_tool_failure() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!("kernel-symbolize-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir(&root).unwrap();
    let capture = root.join("addresses");
    let tool = root.join("addr2line");
    let serial = root.join("serial.log");
    std::fs::write(
        &tool,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$5\" >> '{}'\n",
            capture.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(&serial,b"\xff\ntrap source=Trap pc=0x102\nframe source=Chain pc=0x206\nstack source=Scan pc=0x300\n").unwrap();
    utilities::symbolize(&root.join("kernel.elf"), &serial, tool.to_str().unwrap()).unwrap();
    assert_eq!(std::fs::read_to_string(capture).unwrap(), "0x102\n0x206\n");
    assert!(utilities::symbolize(&root.join("kernel.elf"), &serial, "/bin/false").is_err());
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn composition_rejects_changed_preimages_before_any_edit() {
    let root = std::env::temp_dir().join(format!("kernel-preimage-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let evidence = root.join("evidence");
    std::fs::create_dir_all(&evidence).unwrap();
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let paths = [
        "server/src/entry/boot/mod.rs",
        "server/src/entry/call/debug.rs",
    ];
    for path in paths {
        std::fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
        std::fs::copy(source.join(path), root.join(path)).unwrap();
    }
    let original = std::fs::read(root.join(paths[0])).unwrap();
    std::fs::write(root.join(paths[1]), "unreviewed source").unwrap();
    assert!(composition::apply("room-window", &root, &evidence).is_err());
    assert_eq!(std::fs::read(root.join(paths[0])).unwrap(), original);
    std::fs::copy(source.join(paths[1]), root.join(paths[1])).unwrap();
    composition::apply("room-window", &root, &evidence).unwrap();
    assert_ne!(std::fs::read(root.join(paths[0])).unwrap(), original);
    std::fs::remove_dir_all(root).unwrap();
}
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

mod execution;
