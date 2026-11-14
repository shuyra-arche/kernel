#!/usr/bin/env python3
"""Current ABI A/D guest with explicit capability and real 1/4-Hart evidence."""
import argparse
import difflib
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import time

FIXTURE = "dd32c35dbb67e2e78edb00ad35f968d16423249e"
FLAGS = "-Crelocation-model=static -Cforce-frame-pointers=yes -Ccode-model=medium -Dwarnings"
TARGET = "riscv64gc-unknown-none-elf"


def output(command):
    return subprocess.check_output(command, text=True).strip()


def git(repo, *arguments):
    return ["git", "-c", "safe.directory=" + str(repo.resolve()), "-C", str(repo), *arguments]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def archive(repo, revision, destination, *paths):
    data = subprocess.check_output(git(repo, "archive", revision, *paths))
    with tarfile.open(fileobj=io.BytesIO(data)) as source:
        source.extractall(destination, filter="data")


def compose(provider, fixture, unsupported, stopped):
    suite = provider / "server/tests/guest/activity"
    patches = []

    def edit(root, relative, transform):
        path = root / relative
        before = path.read_text()
        after = transform(before)
        assert before != after, relative
        patches.extend(difflib.unified_diff(before.splitlines(True), after.splitlines(True),
            fromfile="a/" + root.name + "/" + relative,
            tofile="b/" + root.name + "/" + relative))
        path.write_text(after)

    for relative in ["tests/memory/Cargo.toml", "platform/riscv/Cargo.toml"]:
        edit(fixture, relative, lambda s: s.replace(', features = ["memory-v11"]', ''))

    def platform(s):
        begin = s.index("        let external = device::prepare(&tree);")
        end = s.index("        let regions:", begin)
        s = s[:begin] + s[end:]
        s = s.replace("mod device;\n", "").replace("mod window;\n", "")
        s = s.replace("routes: alloc::vec![route]", "routes: alloc::vec![]")
        s = s.replace("            .map(|t| t.with_local_instructions())\n", "")
        if not unsupported:
            s = s.replace("        boot::start()", (
                "        // SAFETY: paired QEMU disables Svade/Svadu: legacy hardware\n"
                "        // A/D updates, coherent atomic PTE RAM, all admitted Harts.\n"
                "        // No hotplug, device writes or unknown ADUE policy.\n"
                "        unsafe { boot::qualify_activity() }.unwrap();\n"
                "        boot::start()"))
        return s

    edit(fixture, "tests/memory/platform/main.rs", platform)
    edit(fixture, "tests/memory/guest/main.rs", lambda _: (suite / "main.rs").read_text().replace(
        "const SUPPORTED: bool = true;", "const SUPPORTED: bool = " + str(not unsupported).lower() + ";").replace(
        "const STOPPED: bool = false;", "const STOPPED: bool = " + str(stopped).lower() + ";"))
    for name in ["cases.rs","reservation.rs"]:
        shutil.copy2(suite / name, fixture / "tests/memory/guest" / name)
    def reservations(s):
        begin=s.index("pub fn reserve_pages(")
        end=s.index("pub fn insert(",begin)
        return s[:begin]+'''pub fn reserve_pages(token:PendId,placement:Placement,access:u64,count:usize)->(MemoPosition,VirtAddr) {
    crate::reservation::reserve(token,ReserveRequest {placement,access,count:PageCount::new(count).unwrap(),level:CellLevel::new(0).unwrap()})
}
'''+s[end:]
    edit(fixture,"tests/memory/guest/support.rs",reservations)
    edit(fixture, "tests/memory/guest/support.rs", lambda s:
        s[:s.index("pub fn image()")] + s[s.index("pub fn accord("):])
    # Rejected Busy has made no mutation; accepted operations retain their IDs.
    edit(fixture, "tests/memory/guest/support.rs", lambda s: s.replace(
        '    let Response::Reply(r) = call(action) else {\n        panic!("mutation reply")\n    };',
        '    let deadline = room::clock()+8_000_000_000;\n'
        '    let r=loop { let Response::Reply(r)=call(action) else {panic!("mutation reply")};\n'
        '        if r.outcome()!=Outcome::Rejected(Failure::Busy){break r;}\n'
        '        assert!(room::clock()<deadline);room::yield_(); };'))
    edit(fixture, "tests/memory/guest/support.rs", lambda s: s.replace(
        'kernel_client::call::unit::create(VirtAddr::new(bytes.as_ptr() as usize)).unwrap()',
        '({ let deadline=room::clock()+8_000_000_000; loop {\n'
        ' match kernel_client::call::unit::create(VirtAddr::new(bytes.as_ptr() as usize)) {\n'
        ' Ok(task)=>break task, Err(e)=>assert_eq!(e.source,kernel_api::abi::call::unit::UnitFail::Busy),\n'
        ' } assert!(room::clock()<deadline);room::yield_(); } })'))
    # All diagnostics and withholding live under tests/, only in this copy.
    edit(provider, "server/src/metal/riscv/mmu/completion.rs", lambda s: s +
        '\n#[path = ' + json.dumps(str(suite / "fences.rs")) + ']\npub(crate) mod activity_fences;\n')
    edit(provider, "server/src/entry/boot/mod.rs", lambda s: s.replace(
        'extern "C" fn run_on_stack() -> ! {',
        'extern "C" fn run_on_stack() -> ! {\n    crate::metal::mmu::completion::activity_fences::run();'))
    edit(provider, "server/src/basis/nest/activity.rs", lambda s: s +
        '\n#[path = ' + json.dumps(str(suite / "probe.rs")) + ']\npub(crate) mod probe;\n')
    edit(provider, "server/src/basis/nest/activity.rs", lambda s: s.replace(
        'pub(crate) fn advance() {', 'pub(crate) fn advance() {\n    let at=probe::stamp();let mut steps=0;').replace(
        '            if s.step() {',
        '            steps+=1;let started=probe::stamp();let done=s.step();probe::step(started);\n            if done {').replace(
        '\n}\n/// Bounded lifecycle hook;', '\n    probe::turn(at,steps);\n}\n/// Bounded lifecycle hook;'))
    edit(provider, "server/src/metal/riscv/mmu/table/mod.rs", lambda s: s.replace(
        'return entry.capture_activity(snapshot).ok_or(NestError::Busy);',
        'let bits=entry.capture_activity(snapshot).ok_or(NestError::Busy)?;\n'
        '                crate::basis::nest::activity::probe::capture();\n                return Ok(bits);'))
    edit(provider, "server/src/metal/riscv/mmu/completion.rs", lambda s: s.replace(
        '    pub(crate) fn begin() -> Result<Self, Error> {',
        '    pub(crate) fn begin() -> Result<Self, Error> {\n        crate::basis::nest::activity::probe::round();').replace(
        '    pub(crate) fn poll(&self) -> bool {',
        '    pub(crate) fn poll(&self) -> bool {\n        crate::basis::nest::activity::probe::poll();'))
    edit(provider, "server/src/entry/trap/mod.rs", lambda s: s.replace(
        '        unsafe { task.capture_extensions(frame.status()) };',
        '        crate::basis::nest::activity::probe::task(task.ident.id.get());\n'
        '        unsafe { task.capture_extensions(frame.status()) };'))
    edit(provider, "server/src/entry/report/shutdown.rs", lambda s: s.replace(
        'pub(crate) fn report() {', 'pub(crate) fn report() {\n    crate::basis::nest::activity::probe::report();'))
    edit(provider,"server/src/metal/riscv/mmu/completion.rs",lambda s:s.replace(
        'interrupt::ipi::send_completion(slot).map_err(|_| Error::Fault)?;',
        'interrupt::ipi::send_completion(slot).map_err(|_| { crate::basis::nest::activity::probe::notify_failed(); Error::Fault })?;').replace(
        '    Err(Error::TimedOut)',
        '    let mut missing=0usize; for id in 0..hart::hart_count() {let slot=hart::HartId::new(id);let (word,bit)=slot.bit();if participants[word]&bit!=0 && !mailboxes[id].covers(round) {missing|=1<<id;}}\n'
        '    crate::basis::nest::activity::probe::timeout(missing,crate::metal::clock::now().as_ticks().wrapping_sub(start));\n    Err(Error::TimedOut)'))
    edit(provider,"server/src/basis/nest/translation/retirement.rs",lambda s:s.replace(
        '    pub(in crate::basis::nest) fn fail(&mut self) {',
        '    pub(in crate::basis::nest) fn fail(&mut self) {\n        crate::basis::nest::activity::probe::failed();'))
    if stopped:
        edit(provider, "server/tests/guest/activity/probe.rs", lambda s: s +
            '\npub(crate) fn stop() { STOPPED.store(true,Relaxed); }\n')
        edit(provider, "server/src/entry/observe/mod.rs", lambda s: s.replace(
            '        crate::basis::nest::activity::advance();',
            '        crate::basis::nest::activity::advance();\n'
            '        if crate::basis::nest::activity::probe::steps()>0 {\n'
            '            crate::basis::nest::activity::probe::stop();break;\n        }'))
    return "".join(patches)


def boot(qemu, firmware, binaries, capsule, destination, harts, unsupported, stopped):
    destination.mkdir()
    image = destination / "initrd.bin"
    image.write_bytes(capsule((binaries / "memory-guest").read_bytes(), 11))
    command = [qemu, "-accel", "tcg,thread=multi", "-machine", "virt,iommu-sys=on",
        "-cpu", ("rv64,svade=on,svadu=off" if unsupported else "rv64,svade=off,svadu=off"), "-smp", str(harts), "-m", "128M", "-nographic",
        "-bios", str(firmware), "-kernel", str(binaries / "memory-platform"), "-initrd", str(image)]
    start = time.monotonic()
    with (destination / "serial.log").open("w") as log, (destination / "stderr.log").open("w") as errors:
        try:
            status = subprocess.run(command, stdout=log, stderr=errors, timeout=120).returncode
        except subprocess.TimeoutExpired:
            status = "timeout"
    serial = (destination / "serial.log").read_text()
    markers = ["PASS activity unsupported=1"] if unsupported else [
        "PASS activity read=1", "PASS activity aliases=3", "PASS activity window=1",
        "PASS activity capacity=16", "PASS activity writers=3",
        "PASS activity readonly=1", "PASS activity consumer-exit=1", "PASS activity large-unsupported=1"]
    if stopped:
        markers = ["TRACE activity-observer-stop", "PASS activity observer-stopped=1"]
    markers += [f"PASS activity-fences harts={harts}", "task: all tasks exited"]
    passed = status == 0 and all(m in serial for m in markers) and not re.search(r"FAIL|\[kernel\] fatal", serial)
    masks = {int(task): int(mask) for task, mask in re.findall(r"TRACE activity-task id=(\d+) mask=(\d+)", serial)}
    writers = [int(task) for task in re.findall(r"TRACE activity-writer task=(\d+)", serial)]
    union = 0
    for task in writers:
        union |= masks.get(task, 0)
    if not unsupported and not stopped:
        passed &= len(writers) == 3 and union.bit_count() >= min(harts, 2)
    budgets = re.findall(r"TRACE activity-budget (.+)", serial)
    metrics = [dict((k, int(v)) for k, v in re.findall(r"(\w+)=(\d+)", row)) for row in budgets]
    passed &= len(metrics) == 1 and metrics[0]["max_steps"] <= 8
    if len(metrics) == 1:
        if unsupported:
            passed &= metrics[0]["captures"] == 0
        elif not stopped:
            passed &= metrics[0]["max_steps"] == 8
    result = {"command": command, "status": status, "passed": bool(passed),
        "elapsed_seconds": time.monotonic() - start, "image_sha256": digest(image),
        "writer_harts": union, "budget": metrics,
        "throughput": re.findall(r"PASS activity writers=3 (.+)", serial)}
    (destination / "result.json").write_text(json.dumps(result, indent=2))
    print(("PASS" if passed else "FAIL"), destination, flush=True)
    return bool(passed)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--kernel", type=Path, required=True)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--profile", choices=["debug", "release"], default="release")
    parser.add_argument("--harts", type=int, nargs="+", choices=[1, 4], default=[1, 4])
    parser.add_argument("--rounds", type=int, default=1)
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument("--unsupported", action="store_true")
    modes.add_argument("--stopped", action="store_true")
    args = parser.parse_args()
    assert args.rounds > 0
    assert output(git(args.fixture, "rev-parse", "HEAD")) == FIXTURE
    root = args.output.resolve()
    root.mkdir(parents=True, exist_ok=False)
    provider, fixture = root / "provider", root / "fixture"
    provider.mkdir()
    fixture.mkdir()
    source = output(git(args.kernel, "rev-parse", "HEAD"))
    archive(args.kernel, source, provider)
    archive(args.fixture, FIXTURE, fixture, "tests/memory", "platform/riscv")
    (root / "composition.diff").write_text(compose(provider, fixture, args.unsupported, args.stopped))
    suite = provider / "server/tests/guest/activity"
    buildroot = fixture / "tests/memory"
    lock = provider / "server/tests/latency/Cargo.lock"
    shutil.copy2(lock, buildroot / "Cargo.lock")
    command = ["cargo", "build", "--locked", "--manifest-path", str(buildroot / "Cargo.toml"), "--target", TARGET]
    for package, folder in [("kernel", "server"), ("kernel-api", "api"), ("kernel-client", "client")]:
        command += ["--config", f'patch."https://github.com/shuyra-arche/kernel.git".{package}.path=' + json.dumps(str(provider / folder))]
    if args.profile == "release":
        command += ["--release"]
    qemu = os.environ.get("QEMU_RISCV64", "qemu-system-riscv64")
    firmware = Path(os.environ["QEMU_OPENSBI"]).resolve()
    metadata = {"source": source, "fixture": FIXTURE, "flags": FLAGS, "build": command,
        "profile": args.profile, "unsupported": args.unsupported, "stopped": args.stopped, "rounds": args.rounds,
        "rustc": output(["rustc", "-Vv"]), "qemu": output([qemu, "--version"]),
        "firmware_sha256": digest(firmware), "composition_sha256": digest(root / "composition.diff"),
        "suite_sha256": {p.name: digest(p) for p in sorted(suite.glob("*")) if p.is_file()},
        "cpu_max": Path("/sys/fs/cgroup/cpu.max").read_text(), "affinity": sorted(os.sched_getaffinity(0))}
    (root / "inputs.json").write_text(json.dumps(metadata, indent=2))
    with (root / "build.log").open("w") as log:
        subprocess.run(command, env=dict(os.environ, RUSTFLAGS=FLAGS), cwd=fixture,
            stdout=log, stderr=subprocess.STDOUT, check=True)
    binaries = buildroot / "target" / TARGET / args.profile
    metadata["sha256"] = {p.name: digest(p) for p in [binaries / "memory-guest", binaries / "memory-platform", buildroot / "Cargo.lock"]}
    (root / "inputs.json").write_text(json.dumps(metadata, indent=2))
    spec = importlib.util.spec_from_file_location("fixture", buildroot / "run.py")
    native = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(native)
    failures = []
    for harts in args.harts:
        for repetition in range(args.rounds):
            destination = root / f"harts-{harts}-{repetition}"
            if not boot(qemu, firmware, binaries, native.capsule, destination, harts, args.unsupported, args.stopped):
                failures.append(str(destination))
    assert not failures, failures


if __name__ == "__main__":
    main()
