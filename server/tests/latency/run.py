#!/usr/bin/env python3
"""Opt-in native Room/IRQ comparison; preserve every composed input and boot."""
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
SUITE = Path(__file__).resolve().parent

def output(command):
    return subprocess.check_output(command, text=True).strip()

def git(repo, *args):
    repo = repo.resolve()
    return ["git", "-c", "safe.directory=" + str(repo), "-C", str(repo), *args]

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def archive(repo, revision, destination, *paths):
    data = subprocess.check_output(git(repo, "archive", revision, *paths))
    with tarfile.open(fileobj=io.BytesIO(data)) as source:
        source.extractall(destination, filter="data")

def compose(args, kernel, destination):
    destination.mkdir()
    provider = destination / "provider"
    fixture = destination / "composition"
    provider.mkdir()
    fixture.mkdir()
    revision = output(git(kernel, "rev-parse", "HEAD"))
    archive(kernel, revision, provider)
    archive(args.fixture, FIXTURE, fixture, "tests/memory", "platform/riscv")
    root = fixture / "tests/memory"
    patches = []
    def edit(path, transform):
        before = path.read_text()
        after = transform(before)
        assert before != after, str(path)
        label = str(path.relative_to(destination))
        patches.extend(difflib.unified_diff(before.splitlines(True), after.splitlines(True), fromfile="a/"+label, tofile="b/"+label))
        path.write_text(after)
    for relative in ["tests/memory", "platform/riscv"]:
        edit(fixture / relative / "Cargo.toml", lambda s: s.replace(', features = ["memory-v11"]', ''))
    if not args.no_latency:
        edit(root / "Cargo.toml", lambda s: s.replace('kernel = { git =', 'kernel = { features = ["latency"], git ='))
        # Test-owned window controls use existing Debug.Put bytes in this copy.
        # No test feature, public diagnostic operation or production hook ships.
        edit(provider / "server/src/entry/boot/mod.rs", lambda s: s.replace('crate::metal::measure::running();', 'let _fixture_controls_window = ();'))
        def bracket(s):
            anchor = '    match crate::metal::console::write(&text[..n]) {'
            assert s.count(anchor) == 1
            return s.replace(anchor, '''    #[cfg(feature = "latency")]
    match &text[..n] {
        b"ROOM:steady\\n" => crate::metal::measure::running(),
        b"ROOM:teardown\\n" => crate::metal::measure::stopping(),
        _ => {}
    }
''' + anchor)
        edit(provider / "server/src/entry/call/debug.rs", bracket)
    if args.mode == "irq":
        # This paired test image explicitly owns the UART aperture. Move its
        # descriptor from ordinary boot authority to the existing trusted MMIO
        # path; do not relax Kernel admission or give the Unit PLIC controls.
        def uart_owner(s):
            anchor = '    unsafe {\n        boot::configure(devices.description())?;\n    }'
            assert s.count(anchor) == 1
            return s.replace(anchor, '''    let mut description = devices.description();
    let mut authorities = alloc::vec::Vec::from(description.resources.authorities);
    let at = authorities.iter().position(|r| r.base == 0x10000000).expect("UART authority");
    let uart = authorities.remove(at);
    let mut external = alloc::vec::Vec::from(description.resources.external);
    external.push(uart);
    description.resources.authorities = &authorities;
    description.resources.external = &external;
    unsafe { boot::configure(description)?; }
''')
        edit(fixture / "platform/riscv/src/boot/mod.rs", uart_owner)
    if args.mode == "realtime":
        # Native's system entry is normally Supervisor and correctly fails RT
        # admission. This isolated boot composition instead loads the same ELF
        # into a genuine User Team through the production builder. The RT
        # admission checks and resident preparation stay unchanged.
        edit(provider / "server/src/basis/unit/team/capsule.rs", lambda s: s.replace('TeamBuilder::new(SpaceKind::Supervisor)', 'TeamBuilder::new(SpaceKind::User)'))
    guest = SUITE / ({"irq": "interrupt.rs", "realtime": "realtime.rs", "room": "guest.rs"}[args.mode])
    edit(root / "guest/main.rs", lambda _: guest.read_text()
        .replace('const ROUNDS: usize = 256;', f'const ROUNDS: usize = {args.events};')
        .replace('const WARMUP_TIMERS: bool = true;', f'const WARMUP_TIMERS: bool = {str(not args.cold_room).lower()};'))
    # These workloads use same-Team Tasks and static boot MMIO, not native RAM
    # Publish or DMA. Leave the provider's SMP qualification gate closed.
    def platform(s):
        assert s.count('.map(|t| t.with_local_instructions())') == 1
        assert s.count('boot::qualify_memory(topology).unwrap();') == 1
        s = s.replace('.map(|t| t.with_local_instructions())', '').replace('boot::qualify_memory(topology).unwrap();', 'let _unqualified_topology = topology;')
        if args.mode == "irq":
            anchor = '        let external = device::prepare(&tree);'
            assert s.count(anchor) == 1
            s = s.replace(anchor, anchor + '''
        // Test image owns this UART for its initial Team's entire lifetime.
        unsafe { boot::grant_mmio(kernel_api::boot::platform::Region::new(0x10000000, 4096)) }.unwrap();
        unsafe { boot::map_entry_mmio(kernel_api::boot::platform::Region::new(0x10000000, 4096), kernel_api::address::VirtAddr::new(0x10000000)) }.unwrap();''')
        return s
    edit(root / "platform/main.rs", platform)
    shutil.copy2(SUITE / "Cargo.lock", root / "Cargo.lock")
    (destination / "composition.diff").write_text("".join(patches))
    command = ["cargo", "build", "--locked", "--manifest-path", str(root / "Cargo.toml"), "--target", TARGET]
    if args.profile == "release": command.append("--release")
    for package, folder in [("kernel", "server"), ("kernel-api", "api"), ("kernel-client", "client")]:
        command += ["--config", f'patch."https://github.com/shuyra-arche/kernel.git".{package}.path={json.dumps(str(provider / folder))}']
    metadata = {"source": revision, "fixture": FIXTURE, "profile": args.profile,
        "mode": args.mode, "latency": not args.no_latency, "cold_room": args.cold_room, "flags": FLAGS, "build": command,
        "rustc": output(["rustc", "-Vv"]), "python": output(["python3", "--version"]),
        "qemu": output([args.qemu, "--version"]).splitlines()[0],
        "cpu_max": Path("/sys/fs/cgroup/cpu.max").read_text(), "affinity": sorted(os.sched_getaffinity(0)),
        "suite_sha256": {p.name: digest(p) for p in [SUITE / "run.py", SUITE / "Cargo.lock", guest]},
        "composition_sha256": digest(destination / "composition.diff")}
    (destination / "inputs.json").write_text(json.dumps(metadata, indent=2))
    with (destination / "build.log").open("w") as log:
        subprocess.run(command, cwd=fixture, env=dict(os.environ, RUSTFLAGS=FLAGS), stdout=log, stderr=subprocess.STDOUT, check=True)
    binaries = root / "target" / TARGET / args.profile
    metadata["sha256"] = {str(p): digest(p) for p in [binaries / "memory-guest", binaries / "memory-platform", args.firmware, root / "Cargo.lock"]}
    (destination / "inputs.json").write_text(json.dumps(metadata, indent=2))
    spec = importlib.util.spec_from_file_location("native_fixture", root / "run.py")
    native = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(native)
    return destination, binaries, native

def boot(args, variant, harts, round_):
    destination, binaries, native = variant
    evidence = destination / f"harts-{harts}-round-{round_}"
    evidence.mkdir()
    image = evidence / "initrd.bin"
    image.write_bytes(native.capsule((binaries / "memory-guest").read_bytes(), 11))
    disk = evidence / "disk.raw"
    disk.write_bytes(bytes(1024 * 1024))
    command = [args.qemu, "-accel", "tcg,thread=multi", "-machine", "virt,iommu-sys=on", "-cpu", "rv64", "-smp", str(harts), "-m", "128M", "-nographic", "-bios", str(args.firmware), "-kernel", str(binaries / "memory-platform"), "-initrd", str(image), "-drive", f"file={disk},format=raw,if=none,id=disk,aio=threads", "-device", "virtio-blk-pci,drive=disk,disable-legacy=on,iommu_platform=on,ats=off,addr=1,ioeventfd=off,vectors=0"]
    started = time.monotonic()
    load = os.getloadavg()
    with (evidence / "serial.log").open("w") as log, (evidence / "stderr.log").open("w") as errors:
        try: status = subprocess.run(command, stdout=log, stderr=errors, timeout=120).returncode
        except subprocess.TimeoutExpired: status = "timeout"
    (evidence / "result.json").write_text(json.dumps({"command": command, "status": status, "elapsed_seconds": time.monotonic()-started, "host_load_start": load, "host_load_end": os.getloadavg(), "image_sha256": digest(image)}, indent=2))
    log = (evidence / "serial.log").read_text()
    markers = ["task: all tasks exited", "ROOM:steady", "ROOM:teardown", "iommu: command engine closed"]
    markers += {"irq": [f"PASS irq rounds={args.events}", "observers=6", "revoke-sign=1 revoke-view=1 ancestor-view=1 rebind=4"], "room": ["PASS room workers=8 turns=512 steps=4096"], "realtime": ["PASS realtime harts=" + str(harts), "unsupported=1" if harts == 1 else "budget=1 replenishment=1 freeze=1 no-credit-reset=1 preemption=1 fixed-hart=1 overload=denied ordinary-pool=restored"]}[args.mode]
    passed = status == 0 and "FAIL" not in log and "[kernel] fatal" not in log and all(m in log for m in markers)
    if not args.no_latency:
        passed &= "latency-window:" in log and "boundary=whole-interval" in log and "phase=running" in log
    print(f"{'PASS' if passed else 'FAIL'} {destination.name} {args.mode} {args.profile} harts={harts} round={round_}: {evidence}", flush=True)
    return passed, str(evidence)

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--kernel", type=Path, required=True)
    parser.add_argument("--baseline", type=Path, help="build both inputs first, then alternate their boot order each round")
    parser.add_argument("--output", type=Path, required=True, help="new evidence directory")
    parser.add_argument("--profile", choices=["debug", "release"], default="release")
    parser.add_argument("--mode", choices=["room", "irq", "realtime"], default="room")
    parser.add_argument("--events", type=int, default=256, help="UART events, use 16 for the bounded correctness check")
    parser.add_argument("--harts", type=int, choices=[1, 4], nargs="+", default=[1, 4])
    parser.add_argument("--rounds", type=int, default=3)
    parser.add_argument("--no-latency", action="store_true")
    parser.add_argument("--cold-room", action="store_true", help="Room first-use experiment: warm Yield only, leaving timed Sleep paths cold")
    args = parser.parse_args()
    assert 1 <= args.rounds <= 100 and 16 <= args.events <= 4096 and len(args.harts) == len(set(args.harts))
    assert not args.cold_room or args.mode == "room"
    assert output(git(args.fixture, "rev-parse", "HEAD")) == FIXTURE
    args.output = args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    args.qemu = os.environ.get("QEMU_RISCV64", "qemu-system-riscv64")
    args.firmware = Path(os.environ["QEMU_OPENSBI"]).resolve()
    assert "version 10.0." in output([args.qemu, "--version"])
    variants = []
    if args.baseline: variants.append(compose(args, args.baseline, args.output / "baseline"))
    variants.append(compose(args, args.kernel, args.output / "candidate"))
    failures = []
    order = []
    for harts in args.harts:
        for round_ in range(args.rounds):
            for variant in variants[::(-1 if round_ % 2 else 1)]:
                passed, path = boot(args, variant, harts, round_)
                order.append(path)
                if not passed: failures.append(path)
    (args.output / "order.json").write_text(json.dumps(order, indent=2))
    assert not failures, failures

if __name__ == "__main__":
    main()
