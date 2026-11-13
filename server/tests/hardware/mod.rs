use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;

#[test]
#[ignore = "requires QEMU RISC-V with virt,iommu-sys support (validated on 10.0.13)"]
fn iommu_on_real_qemu_registers() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("guest/iommu");
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
        .arg(root.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", root.join("target"))
        .output()
        .expect("build guest");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let guest = root.join("target/riscv64gc-unknown-none-elf/release/iommu-guest");
    let qemu = std::env::var_os("QEMU_RISCV64").unwrap_or_else(|| "qemu-system-riscv64".into());
    let version = Command::new(&qemu)
        .arg("--version")
        .output()
        .expect("QEMU is required");
    assert!(version.status.success());
    let hash = Command::new("sha256sum")
        .arg(&guest)
        .output()
        .expect("sha256sum is required");
    assert!(hash.status.success());
    let evidence = root.join("target/evidence");
    // This directory contains only generated results. Do not let a failed run
    // inherit successful scenario logs from an earlier invocation.
    if evidence.exists() {
        std::fs::remove_dir_all(&evidence).unwrap();
    }
    std::fs::create_dir_all(&evidence).unwrap();
    let receipt = format!(
        "{}\n{}\nfirmware=none; guest runs in M-mode\n",
        String::from_utf8_lossy(&version.stdout),
        String::from_utf8_lossy(&hash.stdout)
    );
    std::fs::write(evidence.join("receipt.txt"), receipt).unwrap();
    for (name, machine, args, expected, pass) in [
        (
            "settle",
            "virt,iommu-sys=on",
            "",
            "PASS iommu-command-engine batches=300 returns=4",
            true,
        ),
        (
            "abandon",
            "virt,iommu-sys=on",
            "abandon",
            "PASS iommu-command-engine abandoned=3 returns=0",
            true,
        ),
        (
            "dma",
            "virt,iommu-sys=on",
            "dma",
            "PASS iommu-dma revoked=1 reused=1 denied=3",
            true,
        ),
        (
            "dma-change",
            "virt,iommu-sys=on",
            "dma-change",
            "PASS iommu-dma change=1 revoked=1 reused=1",
            true,
        ),
        (
            "dma-coherent",
            "virt,iommu-sys=on",
            "dma-coherent",
            "PASS iommu-dma coherent-cpu-writer=1 exact-device-retirement=1 reused=1",
            true,
        ),
        (
            "readonly",
            "virt,iommu-sys=on",
            "readonly",
            "PASS iommu-dma readonly=1 denied=5",
            true,
        ),
        (
            "dma-abandon",
            "virt,iommu-sys=on",
            "dma-abandon",
            "PASS iommu-dma abandoned=8 returns=0",
            true,
        ),
        (
            "owner-exit",
            "virt,iommu-sys=on",
            "owner-exit",
            "PASS iommu-dma owner-exit=1 delayed-dma=1 reused=1",
            true,
        ),
        (
            "activation-exit",
            "virt,iommu-sys=on",
            "activation-exit",
            "PASS iommu-dma activation-exit=1 reused=1",
            true,
        ),
        ("missing", "virt", "", "IOMMU missing", false),
        (
            "route-reject",
            "virt,iommu-sys=on",
            "route-reject",
            "PASS iommu-binding rejected=1 returned=8",
            true,
        ),
    ] {
        let mut command = Command::new(&qemu);
        command
            .args([
                "-M", machine, "-cpu", "rv64", "-m", "128M", "-smp", "1", "-bios", "none",
                "-kernel",
            ])
            .arg(&guest)
            .args(["-nographic", "-monitor", "none", "-nic", "none"]);
        if matches!(
            name,
            "dma"
                | "dma-change"
                | "dma-coherent"
                | "readonly"
                | "dma-abandon"
                | "owner-exit"
                | "activation-exit"
                | "route-reject"
        ) {
            command.args([
                "-device",
                "edu,addr=1.0,dma_mask=0xffffffffffffffff",
                "-device",
                "edu,addr=2.0,dma_mask=0xffffffffffffffff",
            ]);
        }
        if !args.is_empty() {
            command.args(["-append", args]);
        }
        std::fs::write(
            evidence.join(format!("{name}-command.txt")),
            format!("{command:?}\n"),
        )
        .unwrap();
        let output = run(command, &evidence, name, Duration::from_secs(60));
        let serial = String::from_utf8_lossy(&output.stdout);
        assert_eq!(output.status.success(), pass, "{name}: {serial}");
        assert!(
            serial.lines().any(|line| line.contains(expected)),
            "{name}: {serial}"
        );
        if pass {
            assert!(!serial.contains("FAIL"), "{name}: {serial}");
        }
    }
    println!("QEMU evidence: {}", evidence.display());
}

fn run(mut command: Command, evidence: &PathBuf, name: &str, watchdog: Duration) -> Output {
    let started = Instant::now();
    let serial = evidence.join(format!("{name}-serial.log"));
    let stderr = evidence.join(format!("{name}-stderr.log"));
    // Keep output flowing while the watchdog waits, even when the guest emits
    // more than a pipe can buffer before shutdown.
    let mut child = command
        .stdout(Stdio::from(std::fs::File::create(&serial).unwrap()))
        .stderr(Stdio::from(std::fs::File::create(&stderr).unwrap()))
        .spawn()
        .expect("start QEMU");
    // Traced debug IRQ/IOMMU images take 24–26 s locally, including firmware,
    // FDT admission and shutdown. Leave host-load headroom; guest deadlines and
    // behavioral assertions remain unchanged and process exit is still required.
    let deadline = started + watchdog;
    let mut timed_out = false;
    while child.try_wait().expect("QEMU status").is_none() {
        if Instant::now() >= deadline {
            timed_out = true;
            child.kill().expect("stop timed-out QEMU");
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = Output {
        status: child.wait().expect("QEMU status"),
        stdout: std::fs::read(serial).expect("QEMU serial output"),
        stderr: std::fs::read(stderr).expect("QEMU stderr"),
    };
    std::fs::write(
        evidence.join(format!("{name}-elapsed.txt")),
        format!(
            "elapsed_ms={} watchdog_ms={} timed_out={timed_out}\n",
            started.elapsed().as_millis(),
            watchdog.as_millis()
        ),
    )
    .unwrap();
    assert!(
        !timed_out,
        "{name}: QEMU deadline; evidence at {}\nserial:\n{}\nstderr:\n{}",
        evidence.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[cfg(target_os = "linux")]
#[test]
fn watchdog_captures_output_larger_than_a_pipe() {
    let evidence = std::env::temp_dir().join(format!("kernel-qemu-output-{}", std::process::id()));
    std::fs::create_dir_all(&evidence).unwrap();
    let mut command = Command::new("sh");
    command.args([
        "-c",
        "head -c 1048576 /dev/zero && head -c 1048576 /dev/zero >&2",
    ]);
    let output = run(command, &evidence, "output", Duration::from_secs(10));
    std::fs::remove_dir_all(evidence).unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout.len(), 1_048_576);
    assert_eq!(output.stderr.len(), 1_048_576);
}

mod image;
mod kernel;

mod context;

use kernel_hardware_fixture::host::{clock, harts, memory};

mod diagnostics;
mod latency;

mod realtime;
