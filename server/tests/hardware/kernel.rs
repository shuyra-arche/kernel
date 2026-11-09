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
#[test]
#[ignore = "requires RVV-capable QEMU and OpenSBI"]
fn task_vector_context_isolated() {
    run("vector")
}
#[test]
#[ignore = "requires RVV-capable QEMU and OpenSBI"]
fn task_migration_respects_hart_capabilities() {
    run("migration")
}
#[test]
#[ignore = "requires RV64 QEMU with Sstc and OpenSBI"]
fn clock_backends_rearm_deadlines() {
    run("clock")
}
#[test]
#[ignore = "requires RV64 QEMU with 3 GiB contiguous guest RAM"]
fn cell_gigabyte_runs_with_contiguous_ram() {
    run("large")
}
#[test]
#[ignore = "requires QEMU virt PLIC and UART"]
fn hardware_interrupt_sign_delivers_and_rebinds() {
    run("irq")
}
#[test]
#[ignore = "requires QEMU virt IOMMU, PLIC, UART and EDU"]
fn hardware_interrupt_sign_coexists_with_iommu() {
    run("irq-io")
}
static RUN: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn run(mode: &str) {
    let _run = RUN.lock().unwrap_or_else(|e| e.into_inner());
    let io = matches!(mode, "io" | "io-exit" | "irq-io");
    let context = matches!(mode, "context" | "vector" | "migration");
    let mixed = mode == "migration";
    let vector = mode == "vector";
    let clock = mode == "clock";
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let guest = workspace.join("server/tests/guest/cell");
    let target = guest.join("target");
    let variable = if io {
        "KERNEL_PLATFORM_MANIFEST"
    } else {
        "KERNEL_BOOT_MANIFEST"
    };
    let fixture = std::env::var_os(variable)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| panic!("{variable} must point to the matching external image manifest"));
    let kernel_target = target.join(if io { "platform" } else { "boot" });
    let binary = if io { "shuyra-qemu" } else { "shuyra" };
    let evidence = target.join("evidence").join(mode);
    if evidence.exists() {
        std::fs::remove_dir_all(&evidence).unwrap();
    }
    std::fs::create_dir_all(&evidence).unwrap();
    let mut guest_build = Command::new("cargo");
    if io || context || clock || mode == "large" || mode == "irq" {
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
        build.arg("--manifest-path").arg(&fixture);
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
        let cases: Vec<_> = if clock {
            let mut cases = vec![
                (4, "-sbi", "rv64,sstc=true"),
                (4, "-sstc", "rv64,sstc=true"),
            ];
            if profile == "debug" {
                cases.extend([
                    (4, "-fallback", "rv64,sstc=true"),
                    (4, "-no-sstc", "rv64,sstc=false"),
                    (4, "-sstc-trace", "rv64,sstc=true"),
                ]);
            }
            cases
        } else if mode == "large" {
            vec![(1, "-3g", "rv64")]
        } else if mixed {
            vec![(4, "-mixed-v", "rv64,v=true,vlen=128,elen=64")]
        } else if vector {
            vec![
                (1, "-v128", "rv64,v=true,vlen=128,elen=64"),
                (4, "-v128", "rv64,v=true,vlen=128,elen=64"),
                (1, "-v256", "rv64,v=true,vlen=256,elen=64"),
                (4, "-v256", "rv64,v=true,vlen=256,elen=64"),
                (1, "-no-v", "rv64,v=false"),
                (1, "-absent-v", "rv64,v=false"),
            ]
        } else {
            cases.map(|(h, s)| (h, s, "rv64")).collect()
        };
        for (harts, suffix, cpu) in cases {
            let name = format!("{profile}-smp{harts}{suffix}");
            let mut command = Command::new(&qemu);
            command
                .args([
                    "-M",
                    if io { "virt,iommu-sys=on" } else { "virt" },
                    "-cpu",
                    cpu,
                    "-m",
                    if mode == "large" { "3G" } else { "128M" },
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
            let trace_path = evidence.join(format!("{name}-migration.log"));
            let symbols = if suffix == "-sstc-trace" {
                super::context::trace_checkpoints(
                    &mut command,
                    &elf,
                    &trace_path,
                    &["clock_handoff_send", "clock_handoff_receive"],
                )
            } else if mode.starts_with("irq") && harts == 4 {
                super::context::trace_checkpoints(
                    &mut command,
                    &elf,
                    &trace_path,
                    &["irq_received"],
                )
            } else if context && harts == 4 {
                super::context::trace(&mut command, &elf, &trace_path, vector || mixed, mixed)
            } else {
                Vec::new()
            };
            if clock && suffix != "-fallback" {
                let dtb = evidence.join(format!("{name}.dtb"));
                let permissions: &[_] = if suffix == "-sbi" {
                    &["shuyra,user-time"]
                } else {
                    &["shuyra,user-time", "shuyra,supervisor-stimecmp"]
                };
                super::clock::fixture(&qemu, &dtb, &kernel, cpu, harts, permissions);
                command.arg("-dtb").arg(dtb);
            }
            if suffix == "-absent-v" {
                let dtb = evidence.join("absent-vector.dtb");
                super::harts::absent_vector_fixture(&qemu, &dtb, harts);
                command.arg("-dtb").arg(dtb);
            }
            if mixed {
                let dtb = evidence.join("mixed-vector.dtb");
                super::harts::vector_fixture(&qemu, &dtb, cpu);
                command.arg("-dtb").arg(dtb);
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
            if clock {
                if profile == "debug" {
                    super::clock::verify(&log, matches!(suffix, "-sstc" | "-sstc-trace"));
                }
                let fast = usize::from(suffix != "-fallback");
                let expected = format!("PASS clock monotonic=1 fast={fast}");
                assert!(log.contains(&expected), "{name}: {log}");
                let handoffs = if fast == 0 {
                    "handoffs=0"
                } else {
                    "handoffs=64"
                };
                assert!(log.contains(handoffs), "{name}: {log}");
                if !symbols.is_empty() {
                    super::context::verify_handoff(&trace_path, &symbols);
                }
            }
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
                "PASS cell-levels contiguous=1 boundary=1 movement=1 split=1 merge=1 stale=1 zero=1 capacity=1",
                "PASS organization flat=1 window=1 transfer=1 share=1 revoke=1 internal-nest=1",
                "PASS room-kernel atomic-wait=1 wake=1 deadline=1",
                "PASS pend-kernel range=1 return=1 inspect=1 collect=1",
                "task: all tasks exited",
            ] {
                assert!(log.contains(marker), "{name}: {log}");
            }
            if mode.starts_with("irq") {
                if !symbols.is_empty() {
                    super::context::verify(&trace_path, &symbols, false);
                }
                assert!(log.contains("PASS interrupt uart=1 retained=1 repeated=24 isolated=1 stale=1 death=1 revoke=1 rebind=1 pair=1 observers=6 forward=1 masked=1"), "{name}: {log}");
            }
            if mode == "large" {
                assert!(
                    log.contains("PASS cell-gigabyte allocation=1 split=1 merge=1 data=1"),
                    "{name}: {log}"
                );
            }
            if mode == "io" {
                assert!(
                    log.contains("PASS cell-io-levels last-leaf=1 retirement=1 split=1"),
                    "{name}: {log}"
                );
            }
            if context {
                assert!(
                    log.contains("PASS context float=32 fcsr=1 initial=1 preempt=1 block=1"),
                    "{name}: {log}"
                );
            }
            if context {
                assert!(
                    log.contains("PASS context faults=2 reuse=16 stop=2"),
                    "{name}: {log}"
                );
                if !symbols.is_empty() {
                    super::context::verify(&trace_path, &symbols, mixed);
                }
            }
            if mixed {
                assert!(
                    log.contains("PASS context migration-state=1 incompatible-harts=2"),
                    "{name}: {log}"
                );
            }
            if vector {
                let marker = match suffix {
                    "-v128" => {
                        "PASS context vector=32 vlenb=16 controls=4 vstart=1 preempt=1 block=1"
                    }
                    "-v256" => {
                        "PASS context vector=32 vlenb=32 controls=4 vstart=1 preempt=1 block=1"
                    }
                    _ => "PASS context vector-absent=1",
                };
                assert!(log.contains(marker), "{name}: {log}");
            }
            if io {
                assert!(
                    log.contains(if mode == "io-exit" {
                        "PASS cell-io-exit owner=dead pending=1 retained=1 late-access=denied rebind=1"
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
