use std::{path::Path, process::Command};

#[test]
#[ignore = "requires RV64 toolchain, QEMU and OpenSBI"]
fn cell_calls_run_through_the_real_kernel_and_cpu_mmu() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let guest = workspace.join("server/tests/guest/cell");
    let target = guest.join("target");
    let evidence = target.join("evidence");
    if evidence.exists() {
        std::fs::remove_dir_all(&evidence).unwrap();
    }
    std::fs::create_dir_all(&evidence).unwrap();
    let output = Command::new("cargo")
        .args([
            "+nightly",
            "build",
            "--locked",
            "--release",
            "--target",
            "riscv64gc-unknown-none-elf",
            "--manifest-path",
        ])
        .arg(guest.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", &target)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let elf = target.join("riscv64gc-unknown-none-elf/release/cell-guest");
    let initrd = target.join("initrd");
    std::fs::write(&initrd, super::image::initrd(&std::fs::read(&elf).unwrap())).unwrap();
    let qemu = std::env::var_os("QEMU_RISCV64").unwrap_or_else(|| "qemu-system-riscv64".into());
    let firmware = std::env::var_os("QEMU_OPENSBI").unwrap_or_else(|| "default".into());
    let version = Command::new(&qemu).arg("--version").output().unwrap();
    assert!(version.status.success());
    std::fs::write(evidence.join("qemu.txt"), &version.stdout).unwrap();
    for (profile, extra, harts) in [
        ("debug", None, &[1, 4][..]),
        ("release", Some("--release"), &[4][..]),
    ] {
        let mut build = Command::new("cargo");
        build.current_dir(&workspace).args([
            "+nightly",
            "build",
            "--locked",
            "-p",
            "kernel",
            "--target",
            "riscv64gc-unknown-none-elf",
        ]);
        build.env("CARGO_TARGET_DIR", workspace.join("target"));
        if let Some(extra) = extra {
            build.arg(extra);
        }
        let output = build.output().unwrap();
        std::fs::write(
            evidence.join(format!("{profile}-build.log")),
            &output.stderr,
        )
        .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let kernel = workspace.join(format!(
            "target/riscv64gc-unknown-none-elf/{profile}/shuyra"
        ));
        let hashes = Command::new("sha256sum")
            .args([&kernel, &elf, &initrd])
            .output()
            .unwrap();
        assert!(hashes.status.success());
        std::fs::write(
            evidence.join(format!("{profile}-sha256.txt")),
            hashes.stdout,
        )
        .unwrap();
        for harts in harts {
            let name = format!("{profile}-smp{harts}");
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
                .arg(&kernel)
                .arg("-initrd")
                .arg(&initrd)
                .args(["-nographic", "-monitor", "none", "-nic", "none"]);
            std::fs::write(
                evidence.join(format!("{name}-command.txt")),
                format!("{command:?}\n"),
            )
            .unwrap();
            let output = super::run(command, &evidence, &name);
            let log = String::from_utf8_lossy(&output.stdout);
            assert!(output.status.success(), "{name}: {log}");
            for marker in [
                "PASS cell-kernel movement=1 memo=1 receipt=1",
                "PASS cell-kernel swap=1 cleanup=1",
                "PASS pend-kernel range=1 return=1 inspect=1 collect=1",
                "task: all tasks exited",
            ] {
                assert!(log.contains(marker), "{name}: {log}");
            }
            for failure in ["FAIL cell-kernel", "panic", "boot failed"] {
                assert!(!log.contains(failure), "{name}: {log}");
            }
        }
    }
    println!("Kernel Cell evidence: {}", evidence.display());
}
