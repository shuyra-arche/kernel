#!/usr/bin/env python3
"""Run the pinned CPU native contract fixture with the exact Kernel provider."""
import argparse
import difflib
import hashlib
import importlib.util
import io
import json
import os
import re
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
    edit("tests/memory/guest/main.rs", lambda s: s.replace("mod virtqueue;", "mod smp;\nmod recovery;")
         .replace("    virtqueue::run();", "    smp::run();")
         .replace("mod ipc;\n", "")
         .replace("    ipc::run();\n", ""))
    edit("tests/memory/guest/support.rs", lambda s:
         s[:s.index("pub fn image()")] + s[s.index("pub fn accord("):])
    # Only rejected requests are retried with a fresh identity. Never turn an
    # accepted Failed/Pending result into a second mutation.
    edit("tests/memory/guest/support.rs", lambda s: s.replace(
        "    let Response::Reply(r) = call(action) else {\n        panic!(\"mutation reply\")\n    };",
        "    let deadline = room::clock() + 5_000_000_000;\n"
        "    let r = loop {\n"
        "        let Response::Reply(r) = call(action) else { panic!(\"mutation reply\") };\n"
        "        if r.outcome() != Outcome::Rejected(Failure::Busy) { break r; }\n"
        "        assert!(room::clock() < deadline, \"rejected Busy did not progress\");\n"
        "        room::yield_();\n"
        "    };"))
    # This case builds a new executable Team through native RX publication.
    # It remains outside the data-only qualification, rather than weakening it.
    edit("tests/memory/guest/acceptance/mod.rs", lambda s:
         s.replace("mod authority;\n", "").replace("    authority::run();\n", ""))
    for relative in ["tests/memory/guest/support.rs", "tests/memory/guest/acceptance/mod.rs"]:
        edit(relative, lambda s: s.replace(
            "match Call::from_request(request(action)) {\n"
            "        Call::Flat(c) => kernel_client::memory::flat::invoke(c),\n"
            "        Call::Nest(c) => kernel_client::memory::nest::invoke(c),\n"
            "    }", "crate::recovery::call(action)"))
    edit("tests/memory/guest/support.rs", lambda s:
         s.replace("crate::recovery::call(action)", "crate::recovery::block(action)"))
    edit("tests/memory/guest/support.rs", lambda s: s.replace(
        "    mutate(Action::Retract {\n        memo,\n        expected: memo_status(memo).generation,\n    });",
        "    crate::recovery::retract(memo);"))
    edit("tests/memory/guest/support.rs", lambda s: s.replace(
        "kernel_client::call::unit::create(VirtAddr::new(bytes.as_ptr() as usize)).unwrap()",
        "({\n            let deadline = room::clock() + 5_000_000_000;\n"
        "            loop {\n"
        "                match kernel_client::call::unit::create(VirtAddr::new(bytes.as_ptr() as usize)) {\n"
        "                    Ok(created) => break created,\n"
        "                    Err(error) => assert_eq!(error.into_source(), kernel_api::abi::call::unit::UnitFail::Busy),\n"
        "                }\n"
        "                assert!(room::clock() < deadline, \"Task admission stayed busy\");\n"
        "                room::yield_();\n"
        "            }\n        })"))
    edit("tests/memory/guest/acceptance/worker.rs", lambda s: s.replace(
        "        self.begin(action);\n        self.finish()",
        "        let deadline = room::clock() + 5_000_000_000;\n"
        "        loop {\n"
        "            self.begin(action);\n"
        "            let response = self.finish()?;\n"
        "            if matches!(action, Action::Block { .. }) && matches!(&response, Response::Reply(r) if r.outcome() == Outcome::Rejected(Failure::Busy)) {\n"
        "                assert!(room::clock() < deadline);\n"
        "                room::yield_();\n"
        "                continue;\n"
        "            }\n"
        "            return Ok(response);\n"
        "        }"))
    edit("tests/memory/guest/acceptance/maintenance.rs", lambda s: s.replace(
        "            pend::revoke(worker.0, token).unwrap();",
        "            let deadline = kernel_client::call::room::clock() + 5_000_000_000;\n"
        "            loop {\n"
        "                match pend::revoke(worker.0, token) {\n"
        "                    Ok(()) => break,\n"
        "                    Err(error) => assert_eq!(error.into_source(), kernel_api::abi::call::pend::PendFail::Busy),\n"
        "                }\n"
        "                assert!(kernel_client::call::room::clock() < deadline);\n"
        "                kernel_client::call::room::yield_();\n"
        "            }"))
    for relative in ["tests/memory/guest/support.rs", "tests/memory/guest/acceptance/worker.rs"]:
        edit(relative, lambda s: s.replace(
            "pend::accord(VirtAddr::new(bytes.as_ptr() as usize)).unwrap()",
            "{\n let deadline = kernel_client::call::room::clock() + 5_000_000_000;\n"
            " loop {\n match pend::accord(VirtAddr::new(bytes.as_ptr() as usize)) {\n"
            " Ok(token) => break token,\n"
            " Err(error) => assert_eq!(error.into_source(), kernel_api::abi::call::pend::PendFail::Busy),\n"
            " }\n assert!(kernel_client::call::room::clock() < deadline);\n"
            " kernel_client::call::room::yield_();\n }\n }"))
    edit("tests/memory/guest/acceptance/maintenance.rs", lambda s: s.replace(
        "        assert!(\n            matches!(worker.call(Action::Publish {memo:local,expected:memo_status(memo).generation,maintenance:Some(held)}),Ok(Response::Reply(r)) if matches!(r.outcome(),Outcome::Committed(_)))\n        );",
        "        publish_held(&worker, local, memo, held);") + '''
fn publish_held(worker: &Worker, local: MemoPosition, observed: MemoPosition, held: Maintenance) {
    let deadline = kernel_client::call::room::clock() + 5_000_000_000;
    loop {
        let Response::Reply(reply) = worker.call(Action::Publish {
            memo: local, expected: memo_status(observed).generation, maintenance: Some(held),
        }).unwrap() else { panic!("Publish") };
        match reply.outcome() {
            Outcome::Committed(_) => return,
            Outcome::Rejected(Failure::Busy | Failure::Stale) => {},
            other => panic!("maintenance Publish: {other:?}"),
        }
        assert!(kernel_client::call::room::clock() < deadline);
        kernel_client::call::room::yield_();
    }
}
''')
    suite = Path(__file__).resolve().parent
    shutil.copy2(suite / "cases.rs", fixture / "tests/memory/guest/smp.rs")
    shutil.copy2(suite / "recovery.rs", fixture / "tests/memory/guest/recovery.rs")
    instructions = provider / "server/tests/guest/smp/instructions.rs"
    edit("server/src/metal/riscv/mmu/completion.rs", lambda s: s +
         '\n#[path = ' + json.dumps(str(instructions)) + ']\npub(crate) mod instructions_probe;\n', provider)
    probe = provider / "server/tests/guest/smp/probe.rs"
    edit("server/src/metal/riscv/mmu/completion.rs", lambda s: s +
         '\n#[path = ' + json.dumps(str(probe)) + ']\npub(crate) mod probe;\n', provider)
    edit("server/src/entry/boot/mod.rs", lambda s: s.replace(
        'extern "C" fn run_on_stack() -> ! {',
        'extern "C" fn run_on_stack() -> ! {\n    crate::metal::mmu::completion::probe::run();\n    crate::metal::mmu::completion::instructions_probe::run();'), provider)
    edit("server/src/entry/trap/mod.rs", lambda s: s.replace(
        '        unsafe { task.capture_extensions(frame.status()) };',
        '        crate::metal::mmu::completion::probe::entered_task(task.ident.id.get());\n'
        '        unsafe { task.capture_extensions(frame.status()) };'), provider)
    edit("server/src/entry/report/shutdown.rs", lambda s: s.replace(
        'pub(crate) fn report() {',
        'pub(crate) fn report() {\n    crate::metal::mmu::completion::probe::report_migrations();'), provider)
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
            with subprocess.Popen(command, stdout=log, stderr=errors) as process:
                while True:
                    try:
                        status = process.wait(timeout=0.2)
                        break
                    except subprocess.TimeoutExpired:
                        serial = (evidence / "serial.log").read_text()
                        if "FAIL" in serial or "[kernel] fatal" in serial:
                            status = "guest-failure"
                        elif time.monotonic() - start >= 120:
                            status = "timeout"
                        else:
                            continue
                        process.kill()
                        process.wait()
                        break
        log = (evidence / "serial.log").read_text()
        markers = ["PASS native-memory", "PASS native-fault", "PASS acceptance-unit",
                   "PASS acceptance-scopes", "PASS acceptance-lineage", "PASS acceptance-maintenance",
                   "PASS smp-completion", "PASS smp-instructions", "PASS smp-retirement",
                   "PASS smp-executable-rejected", "task: all tasks exited"]
        passed = status == 0 and "FAIL" not in log and "[kernel] fatal" not in log and all(m in log for m in markers)
        writers = re.findall(r"PASS smp-writer task=(\d+)", log)
        migrants = set(re.findall(r"TRACE smp-migration task=(\d+)", log))
        passed = passed and len(writers) == 2 and (harts == 1 or all(w in migrants for w in writers))
        (evidence / "result.json").write_text(json.dumps({"command": command, "status": status,
            "elapsed_seconds": time.monotonic() - start, "passed": passed}, indent=2))
        print(f"{'PASS' if passed else 'FAIL'} {args.profile} harts={harts}: {evidence}", flush=True)
        if not passed:
            failed.append(str(evidence))
    assert not failed, failed


if __name__ == "__main__":
    main()
