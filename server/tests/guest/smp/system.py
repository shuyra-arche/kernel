#!/usr/bin/env python3
"""Verify issue #179's pinned, unchanged System runtime with one Kernel source."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import time

from run import FLAGS, TARGET, digest, extract, output

FIXTURE = "7e4f9dc19bdaf4a0e95ff3b557fdc8959b8547d7"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--kernel", type=Path, required=True)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    evidence = args.output.resolve()
    evidence.mkdir(parents=True, exist_ok=False)
    source = output(["git", "-C", str(args.kernel), "rev-parse", "HEAD"])
    assert output(["git", "-C", str(args.fixture), "rev-parse", "HEAD"]) == FIXTURE
    provider, fixture = evidence / "provider", evidence / "fixture"
    provider.mkdir()
    fixture.mkdir()
    extract(args.kernel, source, provider)
    extract(args.fixture, FIXTURE, fixture)
    qemu = os.environ.get("QEMU_RISCV64", "qemu-system-riscv64")
    firmware = Path(os.environ["QEMU_OPENSBI"]).resolve()
    qemu_version = output([qemu, "--version"])
    assert "version 10.0." in qemu_version
    inputs = {"source": source, "fixture": FIXTURE, "rustc": output(["rustc", "-Vv"]),
              "qemu": qemu_version, "firmware_sha256": digest(firmware), "flags": FLAGS,
              "runner_sha256": digest(Path(__file__)), "build": []}
    env = dict(os.environ, RUSTFLAGS=FLAGS)
    runtime = fixture / "tests/guest/runtime"
    with (evidence / "build.log").open("w") as log:
        for name, root, packages in [
            ("guest", runtime, [("kernel-api", "api"), ("kernel-client", "client")]),
            ("platform", runtime / "platform", [("kernel", "server"), ("kernel-api", "api")]),
        ]:
            command = ["cargo", "build", "--release", "--target", TARGET,
                       "--manifest-path", str(root / "Cargo.toml")]
            for package, folder in packages:
                command += ["--config", f'patch."https://github.com/shuyra-arche/kernel.git".{package}.path='
                            + json.dumps(str(provider / folder))]
            for build in [command, command + ["--locked"]]:
                inputs["build"].append(build)
                (evidence / "inputs.json").write_text(json.dumps(inputs, indent=2))
                subprocess.run(build, cwd=provider, env=env, stdout=log,
                               stderr=subprocess.STDOUT, check=True)
            shutil.copyfile(root / "Cargo.lock", evidence / f"{name}.Cargo.lock")
            shutil.copyfile(root / "target" / TARGET / "release" / f"system-runtime-{name}",
                            evidence / f"{name}.elf")
    spec = importlib.util.spec_from_file_location("system_capsule", fixture / "tests/guest/client/run.py")
    capsule = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(capsule)
    image = evidence / "initrd.bin"
    image.write_bytes(capsule.initrd((evidence / "guest.elf").read_bytes()))
    inputs["sha256"] = {name: digest(evidence / name) for name in
                        ["guest.elf", "platform.elf", "initrd.bin", "guest.Cargo.lock", "platform.Cargo.lock"]}
    (evidence / "inputs.json").write_text(json.dumps(inputs, indent=2))
    failed = []
    for harts in [1, 4]:
        command = [qemu, "-accel", "tcg,thread=multi", "-machine", "virt", "-cpu", "rv64",
                   "-smp", str(harts), "-m", "128M", "-nographic", "-monitor", "none", "-nic", "none",
                   "-bios", str(firmware), "-kernel", str(evidence / "platform.elf"), "-initrd", str(image)]
        start = time.monotonic()
        serial = evidence / f"harts-{harts}.log"
        with serial.open("w") as log:
            try:
                status = subprocess.run(command, stdout=log, stderr=subprocess.STDOUT, timeout=120).returncode
            except subprocess.TimeoutExpired:
                status = "timeout"
        log = serial.read_text()
        passed = status == 0 and "FAIL" not in log and "[kernel] fatal" not in log and all(
            marker in log for marker in ["PASS system-runtime", "PASS supply-native",
                                        "PASS system-rpc", "PASS supply-window", "task: all tasks exited"])
        (evidence / f"harts-{harts}.json").write_text(json.dumps({"command": command,
            "status": status, "elapsed_seconds": time.monotonic() - start, "passed": passed}, indent=2))
        print(f"{'PASS' if passed else 'FAIL'} System release harts={harts}: {serial}", flush=True)
        if not passed:
            failed.append(harts)
    assert not failed, failed


if __name__ == "__main__":
    main()
