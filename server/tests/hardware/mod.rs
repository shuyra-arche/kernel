use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

#[test]
#[ignore = "requires QEMU RISC-V with virt,iommu-sys support (validated on 10.0.13)"]
fn command_engine_on_real_qemu_registers() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/iommu");
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
        ("missing", "virt", "", "IOMMU missing", false),
    ] {
        let mut command = Command::new(&qemu);
        command
            .args([
                "-M", machine, "-cpu", "rv64", "-m", "128M", "-smp", "1", "-bios", "none",
                "-kernel",
            ])
            .arg(&guest)
            .args(["-nographic", "-monitor", "none", "-nic", "none"]);
        if !args.is_empty() {
            command.args(["-append", args]);
        }
        std::fs::write(
            evidence.join(format!("{name}-command.txt")),
            format!("{command:?}\n"),
        )
        .unwrap();
        let output = run(command, &evidence, name);
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

fn run(mut command: Command, evidence: &PathBuf, name: &str) -> Output {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start QEMU");
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut timed_out = false;
    while child.try_wait().expect("QEMU status").is_none() {
        if Instant::now() >= deadline {
            timed_out = true;
            child.kill().expect("stop timed-out QEMU");
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = child.wait_with_output().expect("QEMU output");
    std::fs::write(evidence.join(format!("{name}-serial.log")), &output.stdout).unwrap();
    std::fs::write(evidence.join(format!("{name}-stderr.log")), &output.stderr).unwrap();
    assert!(
        !timed_out,
        "{name}: QEMU deadline; evidence at {}",
        evidence.display()
    );
    output
}
