//! Deliberate faults belong to the test image, not production feature switches.
use std::{path::Path, process::Command};
#[test]
#[ignore = "requires RV64 QEMU and OpenSBI with DBCN"]
fn crash_evidence_comes_from_the_fault_and_survives_without_an_allocator() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("guest/diagnostics");
    let target = root.join("target");
    let evidence = target.join("evidence");
    if evidence.exists() {
        std::fs::remove_dir_all(&evidence).unwrap();
    }
    std::fs::create_dir_all(&evidence).unwrap();
    let qemu = std::env::var_os("QEMU_RISCV64").unwrap_or_else(|| "qemu-system-riscv64".into());
    let firmware = std::env::var_os("QEMU_OPENSBI").unwrap_or_else(|| "default".into());
    let version = Command::new(&qemu).arg("--version").output().unwrap();
    assert!(version.status.success());
    std::fs::write(evidence.join("qemu.txt"), version.stdout).unwrap();
    for mode in ["panic", "trap", "early", "recursive"] {
        for profile in ["debug", "release"] {
            let mut build = Command::new("cargo");
            build
                .args([
                    "+nightly",
                    "build",
                    "--locked",
                    "--target",
                    "riscv64gc-unknown-none-elf",
                    "--manifest-path",
                ])
                .arg(root.join("Cargo.toml"))
                .env("CARGO_TARGET_DIR", &target);
            if mode != "panic" {
                build.args(["--features", mode]);
            }
            if profile == "release" {
                build.arg("--release");
            }
            let output = build.output().unwrap();
            std::fs::write(
                evidence.join(format!("{mode}-{profile}-build.log")),
                &output.stderr,
            )
            .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let elf = target.join(format!(
                "riscv64gc-unknown-none-elf/{profile}/diagnostics-guest"
            ));
            std::fs::copy(&elf, evidence.join(format!("{mode}-{profile}.elf"))).unwrap();
            let symbols = Command::new("nm").arg(&elf).output().unwrap();
            assert!(symbols.status.success());
            let symbols = String::from_utf8(symbols.stdout).unwrap();
            let pc = symbols
                .lines()
                .find(|l| l.ends_with(" diagnostics_fault_pc"))
                .map(|l| usize::from_str_radix(l.split_whitespace().next().unwrap(), 16).unwrap());
            let hash = Command::new("sha256sum").arg(&elf).output().unwrap();
            assert!(hash.status.success());
            std::fs::write(
                evidence.join(format!("{mode}-{profile}-sha256.txt")),
                hash.stdout,
            )
            .unwrap();
            for harts in [1, 4] {
                let name = format!("{mode}-{profile}-smp{harts}");
                let mut command = Command::new(&qemu);
                command
                    .args([
                        "-M",
                        "virt",
                        "-cpu",
                        "rv64",
                        "-m",
                        "128M",
                        "-smp",
                        &harts.to_string(),
                        "-bios",
                    ])
                    .arg(&firmware)
                    .arg("-kernel")
                    .arg(&elf)
                    .args(["-nographic", "-monitor", "none", "-nic", "none"]);
                std::fs::write(
                    evidence.join(format!("{name}-command.txt")),
                    format!("{command:?}\n"),
                )
                .unwrap();
                let output = super::run(command, &evidence, &name);
                let serial = String::from_utf8_lossy(&output.stdout);
                assert!(
                    !output.status.success(),
                    "fatal boot must reset with failure: {serial}"
                );
                if mode == "recursive" {
                    assert!(
                        serial.contains("recursive-failure evidence=incomplete"),
                        "{serial}"
                    );
                    continue;
                }
                assert!(serial.contains("root-canary=true"), "{serial}");
                assert!(serial.contains("emergency-lost=0"), "{serial}");
                if mode == "trap" {
                    assert!(
                        serial.contains(&format!("source=Trap pc={:#x} cause=0x2", pc.unwrap())),
                        "{serial}"
                    );
                    assert!(serial.contains("x10=0x12345678"), "{serial}");
                    assert!(!serial.contains("source=RustPanic"), "{serial}");
                } else {
                    assert!(
                        serial.contains("source=RustPanic fault-registers=unavailable"),
                        "{serial}"
                    );
                    assert!(
                        serial.contains(if mode == "early" {
                            "before allocator initialization"
                        } else {
                            "deliberate Rust panic"
                        }),
                        "{serial}"
                    );
                    assert!(!serial.contains("x10="), "{serial}");
                }
                assert!(serial.contains("peers-unobserved="), "{serial}");
            }
        }
    }
}
