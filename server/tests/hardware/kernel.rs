use std::{path::Path, process::Command};

#[test]
#[ignore = "requires RV64 toolchain, QEMU and OpenSBI"]
fn cell_calls_run_through_the_real_kernel_and_cpu_mmu() {
    run("cpu")
}
#[test]
#[ignore = "requires QEMU virt iommu-sys and EDU"]
fn cell_io_calls_run_through_the_real_kernel_and_iommu() {
    run("io")
}
#[test]
#[ignore = "requires QEMU virt iommu-sys and EDU"]
fn cell_io_creator_death_keeps_retirement_until_hardware_settles() {
    run("io-exit")
}
#[test]
#[ignore = "requires RV64 toolchain, QEMU and OpenSBI"]
fn task_extended_context_isolated() {
    run("context")
}
static RUN: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn run(mode: &str) {
    let _run = RUN.lock().unwrap_or_else(|e| e.into_inner());
    let io = matches!(mode, "io" | "io-exit");
    let context = mode == "context";
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let guest = workspace.join("server/tests/guest/cell");
    let target = guest.join("target");
    let fixture = workspace.join("server/tests/platform/qemu/Cargo.toml");
    let (kernel_target, binary) = if io {
        (target.join("platform"), "shuyra-qemu")
    } else {
        (workspace.join("target"), "shuyra")
    };
    let evidence = target.join("evidence").join(mode);
    if evidence.exists() {
        std::fs::remove_dir_all(&evidence).unwrap();
    }
    std::fs::create_dir_all(&evidence).unwrap();
    let mut guest_build = Command::new("cargo");
    if io || context {
        guest_build.args(["+nightly", "build", "--features", mode]);
    } else {
        guest_build.args(["+nightly", "build"]);
    }
    let output = guest_build
        .args([
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
            "--target",
            "riscv64gc-unknown-none-elf",
        ]);
        build.env("CARGO_TARGET_DIR", &kernel_target);
        if io {
            build.arg("--manifest-path").arg(&fixture);
        } else {
            build.args(["-p", "kernel"]);
        }
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
        let kernel = kernel_target.join(format!("riscv64gc-unknown-none-elf/{profile}/{binary}"));
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
        let cases = harts
            .iter()
            .map(|harts| (*harts, ""))
            .chain((profile == "debug" && !io).then_some((4, "-split-memory")))
            .chain((profile == "debug" && mode != "io-exit").then_some((4, "-sparse-harts")));
        for (harts, suffix) in cases {
            let name = format!("{profile}-smp{harts}{suffix}");
            let mut command = Command::new(&qemu);
            command
                .args([
                    "-M",
                    if io { "virt,iommu-sys=on" } else { "virt" },
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
            if io {
                command.args(["-device", "edu,addr=1.0"]);
            }
            if suffix == "-split-memory" {
                let dtb = evidence.join("split-memory.dtb");
                super::memory::fixture(&qemu, &dtb, harts);
                command.arg("-dtb").arg(dtb);
            }
            if suffix == "-sparse-harts" {
                let dtb = evidence.join("sparse-harts.dtb");
                super::harts::fixture(&qemu, &dtb, io);
                command.arg("-dtb").arg(dtb);
            }
            std::fs::write(
                evidence.join(format!("{name}-command.txt")),
                format!("{command:?}\n"),
            )
            .unwrap();
            let output = super::run(command, &evidence, &name);
            let log = String::from_utf8_lossy(&output.stdout);
            assert!(output.status.success(), "{name}: {log}");
            if suffix == "-sparse-harts" {
                assert!(
                    log.contains("cpu: admitted=2 boot=1 paging=Sv39")
                        || log.contains("cpu: admitted=2 boot=3 paging=Sv39"),
                    "{name}: {log}"
                );
            }
            for marker in [
                "PASS cell-kernel movement=1 memo=1 generation=1 freeze=1",
                "PASS cell-kernel swap=1 cleanup=1",
                "PASS room-kernel atomic-wait=1 wake=1 deadline=1",
                "PASS pend-kernel range=1 return=1 inspect=1 collect=1",
                "task: all tasks exited",
            ] {
                assert!(log.contains(marker), "{name}: {log}");
            }
            if context {
                assert!(
                    log.contains("PASS context float=32 fcsr=1 initial=1 preempt=1 block=1"),
                    "{name}: {log}"
                );
            }
            if io {
                assert!(
                    log.contains(if mode == "io-exit" {
                        "PASS cell-io death=1 result=1 retirement=1"
                    } else {
                        "PASS cell-io external=1 readonly=1 fault=1 fenced=1 ownership=1"
                    }),
                    "{name}: {log}"
                );
            }
            for failure in ["FAIL cell-kernel", "panic", "boot failed"] {
                assert!(!log.contains(failure), "{name}: {log}");
            }
        }
    }
    println!("Kernel Cell evidence: {}", evidence.display());
}
