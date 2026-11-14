#!/usr/bin/env python3
"""Compose real Kernel internals with test-owned Flat cases; preserve each boot."""
import argparse
import difflib
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import time

FIXTURE = "dd32c35dbb67e2e78edb00ad35f968d16423249e"
TARGET = "riscv64gc-unknown-none-elf"
FLAGS = "-Crelocation-model=static -Cforce-frame-pointers=yes -Ccode-model=medium -Dwarnings"

def output(command, **kwargs):
    return subprocess.check_output(command, text=True, **kwargs).strip()

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def extract(repo, revision, destination, *paths):
    data = subprocess.check_output(["git", "-C", str(repo), "archive", revision, *paths])
    with tarfile.open(fileobj=io.BytesIO(data)) as archive:
        archive.extractall(destination, filter="data")

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--kernel", required=True, type=Path)
    parser.add_argument("--fixture", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--mode", choices=["contracts", "timing"], default="contracts")
    parser.add_argument("--profile", choices=["debug", "release"], default="debug")
    parser.add_argument("--harts", choices=[1, 4], type=int, nargs="+", default=[1, 4])
    parser.add_argument("--rounds", type=int, default=1)
    args = parser.parse_args()
    assert sys.version_info >= (3, 12)
    assert 1 <= args.rounds <= 20 and len(args.harts) == len(set(args.harts))
    args.kernel = args.kernel.resolve()
    args.output = args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    source = output(["git", "-C", str(args.kernel), "rev-parse", "HEAD"])
    assert output(["git", "-C", str(args.fixture), "rev-parse", "HEAD"]) == FIXTURE
    provider = args.output / "provider"
    provider.mkdir()
    extract(args.kernel, source, provider)
    platform = args.output / "fixture"
    platform.mkdir()
    extract(args.fixture, FIXTURE, platform, "platform/riscv")
    manifest = platform / "platform/riscv/Cargo.toml"
    manifest.write_text(manifest.read_text().replace(', features = ["memory-v11"]', ''))
    suite = Path(__file__).resolve().parent
    composed = provider / "server/tests/guest/flat"
    # The same suite can test a pre-change provider that has no test files yet.
    shutil.copytree(suite, composed, dirs_exist_ok=True,
        ignore=shutil.ignore_patterns("target", "__pycache__"))
    patches = []
    def edit(relative, transform):
        path = provider / relative
        before = path.read_text()
        after = transform(before)
        assert after != before, relative
        patches.extend(difflib.unified_diff(before.splitlines(True), after.splitlines(True),
            fromfile="a/" + relative, tofile="b/" + relative))
        path.write_text(after)
    edit("server/Cargo.toml", lambda s: s.replace('path = "src/lib.rs"', 'path = "tests/guest/flat/provider.rs"')
        .replace('[features]', '[features]\nflat-probe-contracts = []\nflat-probe-timing = []'))
    edit("server/src/basis/flat/mod.rs", lambda s: s +
        '\n#[path = ' + json.dumps(str(composed / "cases/mod.rs")) + ']\npub(crate) mod probe;\n')
    def boot(s):
        anchor = '    crate::basis::unit::lifecycle::tally::rooted();'
        assert s.count(anchor) == 1
        return s.replace(anchor, '    crate::basis::flat::probe::start()?;\n' + anchor)
    edit("server/src/entry/boot/mod.rs", boot)
    def guard(s):
        assert s.count('    _trap: TrapGuard,') == 1
        assert s.count('            _trap: trap,') == 1
        assert s.count('    fn drop(&mut self) {') == 1
        return s.replace('    _trap: TrapGuard,', '    _trap: TrapGuard,\n    probe_start: u64,') \
            .replace('            _trap: trap,', '            _trap: trap,\n            probe_start: crate::basis::flat::probe::lock_enter(lock as *const _ as *const () as usize),') \
            .replace('    fn drop(&mut self) {', '    fn drop(&mut self) {\n        crate::basis::flat::probe::lock_leave(self.probe_start);')
    edit("server/src/metal/riscv/sync/spin/guard.rs", guard)
    (args.output / "composition.diff").write_text("".join(patches))
    command = ["cargo", "build", "--locked", "--manifest-path", str(composed / "Cargo.toml"),
        "--target", TARGET, "--features", "kernel/flat-probe-" + args.mode,
        "--config", 'patch."https://github.com/shuyra-arche/vanllia.git".platform-riscv.path=' + json.dumps(str(platform / "platform/riscv"))]
    if args.profile == "release": command.append("--release")
    env = dict(os.environ, RUSTFLAGS=FLAGS, FLAT_PROBE_PROVIDER="1")
    firmware = Path(os.environ["QEMU_OPENSBI"]).resolve()
    qemu = os.environ.get("QEMU_RISCV64", "qemu-system-riscv64")
    version = output([qemu, "--version"]).splitlines()[0]
    assert "version 10.0." in version
    cpu = Path("/sys/fs/cgroup/cpu.max")
    inputs = {"source": source, "fixture": FIXTURE, "mode": args.mode, "profile": args.profile,
        "python": sys.version, "rustc": output(["rustc", "-Vv"]), "qemu": version,
        "build": command, "flags": FLAGS, "cpu_max": cpu.read_text() if cpu.exists() else None,
        "affinity": sorted(os.sched_getaffinity(0)), "firmware_sha256": digest(firmware),
        "suite_sha256": {str(p.relative_to(suite)): digest(p) for p in suite.rglob("*")
            if p.is_file() and not {"target", "__pycache__"}.intersection(p.relative_to(suite).parts)},
        "composition_sha256": digest(args.output / "composition.diff")}
    (args.output / "inputs.json").write_text(json.dumps(inputs, indent=2))
    with (args.output / "build.log").open("w") as log:
        subprocess.run(command, env=env, cwd=provider, stdout=log, stderr=subprocess.STDOUT, check=True)
    elf = composed / "target" / TARGET / args.profile / "flat-guest"
    inputs.update(elf_sha256=digest(elf), lock_sha256=digest(composed / "Cargo.lock"))
    (args.output / "inputs.json").write_text(json.dumps(inputs, indent=2))
    failures = []
    for harts in args.harts:
        for round_ in range(args.rounds):
            evidence = args.output / f"harts-{harts}-round-{round_}"
            evidence.mkdir()
            command = [qemu, "-accel", "tcg,thread=multi", "-machine", "virt", "-cpu", "rv64",
                "-smp", str(harts), "-m", "128M", "-nographic", "-bios", str(firmware), "-kernel", str(elf)]
            started = time.monotonic()
            with (evidence / "serial.log").open("w") as log, (evidence / "stderr.log").open("w") as errors:
                try: status = subprocess.run(command, stdout=log, stderr=errors, timeout=120).returncode
                except subprocess.TimeoutExpired: status = "timeout"
            (evidence / "result.json").write_text(json.dumps({"command": command,
                "status": status, "elapsed_seconds": time.monotonic() - started}, indent=2))
            log = (evidence / "serial.log").read_text()
            markers = ["PASS flat-" + args.mode, "task: all tasks exited"]
            if args.mode == "contracts":
                markers += ["PASS flat-oom", "PASS flat-reservation", "PASS flat-cancel", "PASS flat-race"]
            passed = status == 0 and "[kernel] fatal" not in log and all(m in log for m in markers)
            if not passed: failures.append(str(evidence))
            print(f"{'PASS' if passed else 'FAIL'} {args.mode} {args.profile} harts={harts} round={round_}: {evidence}", flush=True)
    assert not failures, failures

if __name__ == "__main__":
    main()
