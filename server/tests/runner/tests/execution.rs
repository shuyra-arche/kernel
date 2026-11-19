use kernel_verification::{capsule, process};
use std::{fs, process::Command, thread, time::Duration};
fn scratch(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("vanllia-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir(&root).unwrap();
    root
}
#[test]
fn process_requires_exit_and_full_acceptance() {
    let root = scratch("exit");
    for (name, script, expected) in [
        ("good", "echo PASS", true),
        ("incomplete", "echo START", false),
        ("failed", "echo PASS; exit 3", false),
        ("fatal", "echo PASS; echo FAIL", false),
        ("signal", "echo PASS; kill -TERM $$", false),
    ] {
        let mut result = process::run(
            Command::new("sh").args(["-c", script]),
            &root.join(name),
            Duration::from_secs(2),
        )
        .unwrap();
        assert_eq!(
            process::accept(
                &mut result,
                &fs::read_to_string(root.join(name).join("serial.log")).unwrap(),
                &["PASS".into()]
            ),
            expected
        );
    }
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn deadline_kills_even_after_success_marker() {
    let root = scratch("deadline");
    let result = process::run(
        Command::new("sh").args(["-c", "echo PASS; sleep 30"]),
        &root,
        Duration::from_millis(100),
    )
    .unwrap();
    assert_eq!(result.status, "timeout");
    assert!(!result.success());
    assert!(result.elapsed_seconds < 5.0);
    let cleanup: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("cleanup.json")).unwrap()).unwrap();
    assert_eq!(cleanup["settled"], true);
    fs::remove_dir_all(root).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn successful_leader_cannot_leave_a_background_process() {
    let root = scratch("background");
    let result = process::run(
        Command::new("sh").args(["-c", "sleep 30 & echo PASS; exit 0"]),
        &root,
        Duration::from_secs(2),
    )
    .unwrap();
    let cleanup: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("cleanup.json")).unwrap()).unwrap();
    assert_eq!(result.code, Some(0));
    assert_eq!(result.status, "cleanup-failed");
    assert!(!result.success());
    assert_eq!(cleanup["unexpected_descendants"], true);
    assert_eq!(cleanup["killed"], true);
    assert_eq!(cleanup["settled"], true);
    assert!(cleanup["reaped"].as_u64().unwrap() >= 1);
    fs::remove_dir_all(root).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn nonzero_exit_with_a_background_process_is_not_a_clean_business_result() {
    let root = scratch("background-nonzero");
    let result = process::run(
        Command::new("sh").args(["-c", "sleep 30 & echo PASS; exit 2"]),
        &root,
        Duration::from_secs(2),
    );
    assert!(result.is_err());
    let receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("result.json")).unwrap()).unwrap();
    let cleanup: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("cleanup.json")).unwrap()).unwrap();
    assert_eq!(receipt["status"], "exited");
    assert_eq!(receipt["code"], 2);
    assert_eq!(receipt["passed"], false);
    assert_eq!(cleanup["unexpected_descendants"], true);
    assert_eq!(cleanup["settled"], true);
    assert!(cleanup["reaped"].as_u64().unwrap() >= 1);
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn large_output_cannot_deadlock_pipes() {
    let root = scratch("streams");
    let result = process::run(
        Command::new("sh").args([
            "-c",
            "head -c 1048576 /dev/zero; head -c 1048576 /dev/zero >&2",
        ]),
        &root,
        Duration::from_secs(2),
    )
    .unwrap();
    assert!(result.success());
    assert_eq!(
        fs::metadata(root.join("serial.log")).unwrap().len(),
        1048576
    );
    assert_eq!(
        fs::metadata(root.join("stderr.log")).unwrap().len(),
        1048576
    );
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn malformed_elf_never_panics_or_builds_an_image() {
    for size in 0..128 {
        assert!(capsule::initrd(&vec![0; size], 13).is_err());
    }
    let mut elf = vec![0; 120];
    elf[..6].copy_from_slice(b"\x7fELF\x02\x01");
    elf[18..20].copy_from_slice(&243u16.to_le_bytes());
    elf[32..40].copy_from_slice(&64u64.to_le_bytes());
    elf[54..56].copy_from_slice(&56u16.to_le_bytes());
    elf[56..58].copy_from_slice(&1u16.to_le_bytes());
    elf[64..68].copy_from_slice(&1u32.to_le_bytes());
    elf[104..112].copy_from_slice(&4096u64.to_le_bytes());
    let image = capsule::initrd(&elf, 13).unwrap();
    assert_eq!(&image[4096..4104], b"SQBOOT01");
    assert_eq!(image[4107], 13);
    let rejected = capsule::initrd(&elf, 10).unwrap();
    assert_eq!(rejected[4107], 10);
    elf[68..72].copy_from_slice(&6u32.to_le_bytes());
    let dense13 = capsule::system_initrd(&elf, 13).unwrap();
    assert_eq!(
        u64::from_le_bytes(dense13[4096 + 48..4096 + 56].try_into().unwrap()),
        1
    );
    assert_eq!(dense13[4107], 13);
    assert_eq!(dense13.len(), 12288);
    let dense14 = capsule::system_initrd(&elf, 14).unwrap();
    assert_eq!(dense14[4107], 14);
    assert_eq!(dense14.len(), dense13.len());
    assert_eq!(&dense14[..4107], &dense13[..4107]);
    assert_eq!(&dense14[4108..], &dense13[4108..]);
    assert!(capsule::system_initrd(&elf, 15).is_err());
    elf[96..104].copy_from_slice(&4097u64.to_le_bytes());
    assert!(capsule::initrd(&elf, 13).is_err());
    elf[32..40].copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(capsule::initrd(&elf, 13).is_err());
}
#[test]
fn failed_launch_preserves_a_failed_result() {
    let root = scratch("spawn");
    assert!(
        process::run(
            &mut Command::new("/nonexistent/vanllia-qemu"),
            &root,
            Duration::from_secs(1)
        )
        .is_err()
    );
    let result: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("result.json")).unwrap()).unwrap();
    assert_eq!(result["status"], "spawn-error");
    assert_eq!(result["passed"], false);
    let cleanup: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("cleanup.json")).unwrap()).unwrap();
    assert_eq!(cleanup["spawned"], false);
    assert_eq!(cleanup["settled"], true);
    fs::remove_dir_all(root).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn runner_cancellation_signals_the_direct_child() {
    if let Some(marker) = std::env::var_os("KERNEL_VERIFICATION_PARENT_DEATH_MARKER") {
        let child_dir = std::env::var_os("KERNEL_VERIFICATION_CHILD_DIR")
            .expect("parent passes the child log directory");
        let child_dir = std::path::PathBuf::from(child_dir);
        let mut command = Command::new("sh");
        command.args([
            "-c",
            r#"echo child-started; sleep 3; touch "$KERNEL_VERIFICATION_PARENT_DEATH_MARKER""#,
        ]);
        command.env("KERNEL_VERIFICATION_PARENT_DEATH_MARKER", marker);
        let _ = process::run(&mut command, &child_dir, Duration::from_secs(10));
        return;
    }

    let root = scratch("cancel-runner");
    let marker = root.join("orphan-finished");
    let child_dir = root.join("child");
    let mut test_child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "execution::runner_cancellation_signals_the_direct_child",
        ])
        .env("KERNEL_VERIFICATION_PARENT_DEATH_MARKER", &marker)
        .env("KERNEL_VERIFICATION_CHILD_DIR", &child_dir)
        .spawn()
        .unwrap();
    for _ in 0..100 {
        if fs::read_to_string(child_dir.join("serial.log"))
            .is_ok_and(|serial| serial.contains("child-started"))
        {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        fs::read_to_string(child_dir.join("serial.log"))
            .is_ok_and(|serial| serial.contains("child-started")),
        "process helper child did not start"
    );
    test_child.kill().unwrap();
    test_child.wait().unwrap();
    thread::sleep(Duration::from_millis(3_200));
    assert!(
        !marker.exists(),
        "direct child continued after runner cancellation"
    );
    fs::remove_dir_all(root).unwrap();
}
