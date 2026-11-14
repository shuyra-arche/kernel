#!/usr/bin/env python3
"""Run the pinned CPU native contract fixture with the exact Kernel provider."""
import argparse
import difflib
import hashlib
import importlib.util
import io
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


def output(command):
    return subprocess.check_output(command, text=True).strip()


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def extract(repo, revision, destination):
    data = subprocess.check_output(["git", "-C", str(repo), "archive", revision])
    with tarfile.open(fileobj=io.BytesIO(data)) as archive:
        archive.extractall(destination, filter="data")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--kernel", type=Path, required=True)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--profile", choices=["debug", "release"], default="debug")
    parser.add_argument("--harts", type=int, nargs="+", choices=[1, 2, 4], default=[1, 2, 4])
    args = parser.parse_args()
    args.output = args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    source = output(["git", "-C", str(args.kernel), "rev-parse", "HEAD"])
    assert output(["git", "-C", str(args.fixture), "rev-parse", "HEAD"]) == FIXTURE
    provider, fixture = args.output / "provider", args.output / "fixture"
    provider.mkdir()
    fixture.mkdir()
    extract(args.kernel, source, provider)
    extract(args.fixture, FIXTURE, fixture)
    patches = []

    def edit(relative, transform, root=fixture):
        path = root / relative
        before = path.read_text()
        after = transform(before)
        assert after != before, relative
        patches.extend(difflib.unified_diff(before.splitlines(True), after.splitlines(True),
            fromfile="a/" + root.name + "/" + relative,
            tofile="b/" + root.name + "/" + relative))
        path.write_text(after)

    for relative in ["tests/memory/Cargo.toml", "platform/riscv/Cargo.toml"]:
        edit(relative, lambda s: s.replace(', features = ["memory-v11"]', ''))
    # CPU-only composition. Never widen the fixture's independent device proof.
    def platform(s):
        begin = s.index("        let external = device::prepare(&tree);")
        end = s.index("        let regions:", begin)
        s = s[:begin] + s[end:]
        s = s.replace("mod device;\n", "").replace("mod window;\n", "")
        s = s.replace("routes: alloc::vec![route]", "routes: alloc::vec![]")
        s = s.replace("            .map(|t| t.with_local_instructions())\n", "")
        begin = s.index("        // SAFETY:")
        end = s.index("        let topology", begin)
        return s[:begin] + (
            "        // SAFETY: paired QEMU virt 1/2/4 Hart CPU-only test, 128 MiB\n"
            "        // coherent RAM, no executable Publish, device route or hotplug.\n"
            "        assert!(matches!(tree.cpus().count(), 1 | 2 | 4));\n"
        ) + s[end:]
    edit("tests/memory/platform/main.rs", platform)
    edit("tests/memory/guest/main.rs", lambda s: s.replace("mod virtqueue;", "mod smp;")
         .replace("    virtqueue::run();", "    smp::run();")
         .replace("mod ipc;\n", "")
         .replace("    ipc::run();\n", ""))
    edit("tests/memory/guest/support.rs", lambda s:
         s[:s.index("pub fn image()")] + s[s.index("pub fn accord("):])
    # This case builds a new executable Team through native RX publication.
    # It remains outside the data-only qualification, rather than weakening it.
    edit("tests/memory/guest/acceptance/mod.rs", lambda s:
         s.replace("mod authority;\n", "").replace("    authority::run();\n", ""))
    suite = Path(__file__).resolve().parent
    shutil.copy2(suite / "cases.rs", fixture / "tests/memory/guest/smp.rs")
    probe = provider / "server/tests/guest/smp/probe.rs"
    edit("server/src/metal/riscv/mmu/completion.rs", lambda s: s +
         '\n#[path = ' + json.dumps(str(probe)) + ']\npub(crate) mod probe;\n', provider)
    edit("server/src/entry/boot/mod.rs", lambda s: s.replace(
        'extern "C" fn run_on_stack() -> ! {',
        'extern "C" fn run_on_stack() -> ! {\n    crate::metal::mmu::completion::probe::run();'), provider)
    (args.output / "composition.diff").write_text("".join(patches))
    manifest = fixture / "tests/memory/Cargo.toml"
    command = ["cargo", "build", "--manifest-path", str(manifest), "--target", TARGET]
    for package, folder in [("kernel", "server"), ("kernel-api", "api"), ("kernel-client", "client")]:
        command += ["--config", f'patch."https://github.com/shuyra-arche/kernel.git".{package}.path='
                    + json.dumps(str(provider / folder))]
    if args.profile == "release":
        command.append("--release")
    env = dict(os.environ, RUSTFLAGS=FLAGS)
    qemu = os.environ.get("QEMU_RISCV64", "qemu-system-riscv64")
    firmware = Path(os.environ["QEMU_OPENSBI"]).resolve()
    qemu_version = output([qemu, "--version"])
    assert "version 10.0." in qemu_version
    inputs = {"source": source, "fixture": FIXTURE, "profile": args.profile,
              "rustc": output(["rustc", "-Vv"]), "qemu": qemu_version,
              "firmware_sha256": digest(firmware), "flags": FLAGS, "build": command,
              "suite_sha256": {p.name: digest(p) for p in suite.iterdir() if p.is_file()},
              "composition_sha256": digest(args.output / "composition.diff")}
    (args.output / "inputs.json").write_text(json.dumps(inputs, indent=2))
    with (args.output / "build.log").open("w") as log:
        # Resolve only the explicit provider path substitutions, then verify the
        # resulting graph with --locked. Both commands and the lock are retained.
        subprocess.run(command, cwd=provider, env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
        subprocess.run(command + ["--locked"], cwd=provider, env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
    lock = fixture / "tests/memory/Cargo.lock"
    shutil.copy2(lock, args.output / "Cargo.lock")
    binaries = fixture / "tests/memory/target" / TARGET / args.profile
    guest, platform_elf = binaries / "memory-guest", binaries / "memory-platform"
    spec = importlib.util.spec_from_file_location("native_capsule", fixture / "tests/memory/run.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    initrd = args.output / "initrd.bin"
    initrd.write_bytes(module.capsule(guest.read_bytes(), 11))
    inputs["sha256"] = {str(p.relative_to(args.output)): digest(p) for p in [guest, platform_elf, initrd, lock]}
    (args.output / "inputs.json").write_text(json.dumps(inputs, indent=2))
    failed = []
    for harts in args.harts:
        evidence = args.output / f"harts-{harts}"
        evidence.mkdir()
        command = [qemu, "-accel", "tcg,thread=multi", "-machine", "virt", "-cpu", "rv64",
                   "-smp", str(harts), "-m", "128M", "-nographic", "-nic", "none",
                   "-bios", str(firmware), "-kernel", str(platform_elf), "-initrd", str(initrd)]
        start = time.monotonic()
        with (evidence / "serial.log").open("w") as log, (evidence / "stderr.log").open("w") as errors:
            try:
                status = subprocess.run(command, stdout=log, stderr=errors, timeout=120).returncode
            except subprocess.TimeoutExpired:
                status = "timeout"
        log = (evidence / "serial.log").read_text()
        markers = ["PASS native-memory", "PASS native-fault", "PASS acceptance-unit",
                   "PASS acceptance-scopes", "PASS acceptance-lineage", "PASS acceptance-maintenance",
                   "PASS smp-completion", "PASS smp-retirement",
                   "PASS smp-executable-rejected", "task: all tasks exited"]
        passed = status == 0 and "FAIL" not in log and "[kernel] fatal" not in log and all(m in log for m in markers)
        (evidence / "result.json").write_text(json.dumps({"command": command, "status": status,
            "elapsed_seconds": time.monotonic() - start, "passed": passed}, indent=2))
        print(f"{'PASS' if passed else 'FAIL'} {args.profile} harts={harts}: {evidence}", flush=True)
        if not passed:
            failed.append(str(evidence))
    assert not failed, failed


if __name__ == "__main__":
    main()
