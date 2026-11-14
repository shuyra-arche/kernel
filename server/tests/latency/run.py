#!/usr/bin/env python3
"""Opt-in ABI 11 Room exploration; never part of the default CI workload."""
import argparse
import hashlib
import importlib.util
import io
import itertools
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import time

FIXTURE = "dd32c35dbb67e2e78edb00ad35f968d16423249e"
TARGET = "riscv64gc-unknown-none-elf"
FLAGS = "-Crelocation-model=static -Cforce-frame-pointers=yes -Ccode-model=medium -Dwarnings"

def output(command, **kwargs):
    return subprocess.check_output(command, text=True, **kwargs).strip()

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--kernel", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True, help="new evidence directory")
    parser.add_argument("--profile", choices=["debug", "release"], default="release")
    parser.add_argument("--harts", type=int, choices=[1, 4], nargs="+", default=[1, 4])
    parser.add_argument("--rounds", type=int, default=3)
    parser.add_argument("--threads", choices=["single", "multi"], default="multi")
    parser.add_argument("--no-latency", action="store_true", help="measure diagnostics overhead with the same workload")
    args = parser.parse_args()
    assert 1 <= args.rounds <= 100
    assert output(["git", "-C", str(args.fixture), "rev-parse", "HEAD"]) == FIXTURE
    args.kernel = args.kernel.resolve()
    args.output = args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    fixture = args.output / "composition"
    fixture.mkdir()
    archive = subprocess.check_output(["git", "-C", str(args.fixture), "archive", FIXTURE, "tests/memory", "platform/riscv"])
    with tarfile.open(fileobj=io.BytesIO(archive)) as source:
        source.extractall(fixture, filter="data")
    root = fixture / "tests/memory"
    for relative in ["tests/memory", "platform/riscv"]:
        manifest = fixture / relative / "Cargo.toml"
        text = manifest.read_text().replace(', features = ["memory-v11"]', '')
        if relative == "tests/memory" and not args.no_latency:
            text = text.replace('kernel = { git =', 'kernel = { features = ["latency"], git =')
        manifest.write_text(text)
    guest = Path(__file__).with_name("guest.rs")
    shutil.copy2(guest, root / "guest/main.rs")
    platform = root / "platform/main.rs"
    # This workload only creates Tasks in the already-loaded Team. It never
    # publishes executable pages, creates a second Team or exercises device DMA.
    # Leave qualified memory unavailable: the provider deliberately rejects
    # multi-Hart drain qualification. This Room-only guest needs no native
    # Publish, DMA or executable-memory capability. Do not weaken that check.
    text = platform.read_text()
    assert text.count('.map(|t| t.with_local_instructions())') == 1
    text = text.replace('.map(|t| t.with_local_instructions())', '')
    assert text.count('boot::qualify_memory(topology).unwrap();') == 1
    text = text.replace('boot::qualify_memory(topology).unwrap();', 'let _unqualified_topology = topology;')
    platform.write_text(text)
    env = dict(os.environ, RUSTFLAGS=FLAGS)
    command = ["cargo", "build", "--manifest-path", str(root / "Cargo.toml"), "--target", TARGET]
    if args.profile == "release": command.append("--release")
    for package, folder in [("kernel", "server"), ("kernel-api", "api"), ("kernel-client", "client")]:
        command += ["--config", f'patch."https://github.com/shuyra-arche/kernel.git".{package}.path={json.dumps(str(args.kernel / folder))}']
    with (args.output / "build.log").open("w") as log:
        subprocess.run(command, cwd=fixture, env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
    spec = importlib.util.spec_from_file_location("native_fixture", root / "run.py")
    native = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(native)
    binaries = root / "target" / TARGET / args.profile
    firmware = Path(os.environ["QEMU_OPENSBI"]).resolve()
    qemu = os.environ.get("QEMU_RISCV64", "qemu-system-riscv64")
    version = output([qemu, "--version"]).splitlines()[0]
    assert "version 10.0." in version
    source = output(["git", "-C", str(args.kernel), "rev-parse", "HEAD"])
    (args.output / "source.diff").write_text(output(["git", "-C", str(args.kernel), "diff", "HEAD"]))
    cpu = Path("/sys/fs/cgroup/cpu.max")
    metadata = {"source": source, "fixture": FIXTURE, "rustc": output(["rustc", "-Vv"]),
        "qemu": version, "cpu_max": cpu.read_text() if cpu.exists() else None,
        "affinity": sorted(os.sched_getaffinity(0)), "flags": FLAGS, "build": command,
        "profile": args.profile, "latency": not args.no_latency, "harts": args.harts, "threads": args.threads,
        "sha256": {str(p.relative_to(args.output)) if p.is_relative_to(args.output) else str(p): digest(p)
            for p in [binaries / "memory-guest", binaries / "memory-platform", firmware, root / "Cargo.lock", guest, platform]}}
    (args.output / "inputs.json").write_text(json.dumps(metadata, indent=2))
    failures = []
    for harts, round_ in itertools.product(args.harts, range(args.rounds)):
        evidence = args.output / f"harts-{harts}-round-{round_}"
        evidence.mkdir()
        image = evidence / "initrd.bin"
        image.write_bytes(native.capsule((binaries / "memory-guest").read_bytes(), 11))
        disk = evidence / "disk.raw"
        disk.write_bytes(bytes(1024 * 1024))
        command = [qemu, "-accel", f"tcg,thread={args.threads}", "-machine", "virt,iommu-sys=on", "-cpu", "rv64",
            "-smp", str(harts), "-m", "128M", "-nographic", "-bios", str(firmware), "-kernel", str(binaries / "memory-platform"), "-initrd", str(image),
            "-drive", f"file={disk},format=raw,if=none,id=disk,aio=threads",
            "-device", "virtio-blk-pci,drive=disk,disable-legacy=on,iommu_platform=on,ats=off,addr=1,ioeventfd=off,vectors=0"]
        started = time.monotonic()
        with (evidence / "serial.log").open("w") as log, (evidence / "stderr.log").open("w") as errors:
            try:
                result = subprocess.run(command, stdout=log, stderr=errors, timeout=120)
                status = result.returncode
            except subprocess.TimeoutExpired:
                status = "timeout"
        (evidence / "result.json").write_text(json.dumps({"command": command, "status": status,
            "elapsed_seconds": time.monotonic() - started, "image_sha256": digest(image)}, indent=2))
        log = (evidence / "serial.log").read_text()
        passed = status == 0 and "FAIL" not in log and "PASS room workers=8 turns=512 steps=4096" in log and "task: all tasks exited" in log
        if not passed: failures.append(str(evidence))
        print(f"{'PASS' if passed else 'FAIL'} {args.profile} harts={harts} round={round_}: {evidence}", flush=True)
    assert not failures, failures

if __name__ == "__main__":
    main()
