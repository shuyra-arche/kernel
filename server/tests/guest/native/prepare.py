#!/usr/bin/env python3
"""Adapt the pinned native fixture without changing accepted-request semantics."""
import argparse
import difflib
import json
from pathlib import Path
import subprocess

FIXTURE = "dd32c35dbb67e2e78edb00ad35f968d16423249e"


def prepare(fixture):
    revision = subprocess.check_output(
        ["git", "-C", str(fixture), "rev-parse", "HEAD"], text=True
    ).strip()
    assert revision == FIXTURE, "unreviewed native fixture"
    patches = []

    def edit(relative, transform):
        path = fixture / relative
        before = path.read_text()
        after = transform(before)
        assert before != after, relative
        patches.extend(difflib.unified_diff(
            before.splitlines(True), after.splitlines(True),
            fromfile="a/" + relative, tofile="b/" + relative,
        ))
        path.write_text(after)

    for relative in ["tests/memory/Cargo.toml", "platform/riscv/Cargo.toml"]:
        edit(relative, lambda s: s.replace(', features = ["memory-v11"]', ''))

    def release(source):
        begin = source.index("fn release(memo: MemoPosition) {")
        end = source.index("\n#[derive(Clone, Copy, Debug)]", begin)
        old = source[begin:end]
        assert old.count("mutate(Action::Release {") == 1
        assert old.count("expected: s.version,") == 1
        # Hardware settlement can advance a Reservation between observation and
        # admission. A rejected invocation has no effects and permits a fresh
        # request after observing the current version. Never replay accepted
        # Pending/Failed work as a fresh mutation or pretend it has completed.
        replacement = '''fn release(memo: MemoPosition) {
    let deadline = room::clock() + 3_000_000_000;
    loop {
        let Snapshot::Reservation(s) = inspect(Query::Reservation {
            token: memo.token(),
            nest: memo.nest(),
            reservation: memo.reservation(),
        }) else {
            panic!("Reservation")
        };
        let Response::Reply(reply) = call(Action::Release {
            token: memo.token(),
            nest: memo.nest(),
            reservation: memo.reservation(),
            expected: s.version,
        }) else {
            panic!("Release reply")
        };
        match reply.outcome() {
            Outcome::Committed(_) => return,
            Outcome::Rejected(Failure::Stale) => {
                assert!(room::clock() < deadline, "Release version did not settle");
                room::yield_();
            }
            other => panic!("Release: {other:?}"),
        }
    }
}
'''
        return source[:begin] + replacement + source[end:]

    edit("tests/memory/guest/virtqueue.rs", release)
    evidence = fixture / "tests/memory/target/evidence"
    evidence.mkdir(parents=True, exist_ok=True)
    (evidence / "composition.diff").write_text("".join(patches))
    (evidence / "composition.json").write_text(json.dumps({
        "fixture": revision,
        "adaptations": ["retired memory-v11 feature", "reobserve rejected Release Stale only"],
    }, indent=2))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fixture", type=Path, required=True)
    prepare(parser.parse_args().fixture.resolve())
