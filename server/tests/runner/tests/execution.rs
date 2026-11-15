use kernel_verification::{capsule, process};
use std::{fs, process::Command, time::Duration};
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
        assert!(capsule::initrd(&vec![0; size], 11).is_err());
    }
    let mut elf = vec![0; 120];
    elf[..6].copy_from_slice(b"\x7fELF\x02\x01");
    elf[18..20].copy_from_slice(&243u16.to_le_bytes());
    elf[32..40].copy_from_slice(&64u64.to_le_bytes());
    elf[54..56].copy_from_slice(&56u16.to_le_bytes());
    elf[56..58].copy_from_slice(&1u16.to_le_bytes());
    elf[64..68].copy_from_slice(&1u32.to_le_bytes());
    elf[104..112].copy_from_slice(&4096u64.to_le_bytes());
    let image = capsule::initrd(&elf, 11).unwrap();
    assert_eq!(&image[4096..4104], b"SQBOOT01");
    assert_eq!(image[4107], 11);
    let rejected = capsule::initrd(&elf, 10).unwrap();
    assert_eq!(rejected[4107], 10);
    elf[68..72].copy_from_slice(&6u32.to_le_bytes());
    let dense = capsule::system_initrd(&elf).unwrap();
    assert_eq!(
        u64::from_le_bytes(dense[4096 + 48..4096 + 56].try_into().unwrap()),
        1
    );
    assert_eq!(dense.len(), 12288);
    elf[96..104].copy_from_slice(&4097u64.to_le_bytes());
    assert!(capsule::initrd(&elf, 11).is_err());
    elf[32..40].copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(capsule::initrd(&elf, 11).is_err());
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
    fs::remove_dir_all(root).unwrap();
}
