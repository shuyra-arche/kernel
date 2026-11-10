#!/usr/bin/env python3
"""Partition the ignored Cargo tests without changing their scenarios.

Run one group with `python3 server/tests/hardware/shard.py memory`.
Every invocation checks the complete compiled inventory before selecting tests.
Groups run on independent CI runners; keep the Rust harness's shared-image lock.
"""

import argparse
from collections import Counter
from pathlib import Path
import subprocess
import sys


SHARDS = {
    "memory": (
        "diagnostics::crash_evidence_comes_from_the_fault_and_survives_without_an_allocator",
        "kernel::cell_calls_run_through_the_real_kernel_and_cpu_mmu",
        "kernel::cell_gigabyte_runs_with_contiguous_ram",
    ),
    "io": (
        "iommu_on_real_qemu_registers",
        "kernel::cell_io_calls_run_through_the_real_kernel_and_iommu",
        "kernel::cell_io_creator_death_keeps_retirement_until_hardware_settles",
        "kernel::hardware_interrupt_sign_coexists_with_iommu",
    ),
    "scheduling": (
        "kernel::clock_backends_rearm_deadlines",
        "kernel::hardware_interrupt_sign_delivers_and_rebinds",
        "kernel::latency_baseline_observes_contended_paths",
        "kernel::realtime_reservations_and_service_dependencies",
    ),
    "context": (
        "kernel::task_extended_context_isolated",
        "kernel::task_migration_respects_hart_capabilities",
        "kernel::task_vector_context_isolated",
    ),
}
ROOT = Path(__file__).resolve().parents[3]
CARGO = (
    "cargo", "+nightly", "test", "--locked", "--workspace",
    "--target", "x86_64-unknown-linux-gnu",
)


def inventory(output: str) -> tuple[str, ...]:
    """Read libtest's terse listing, not Cargo diagnostics or test summaries."""
    return tuple(
        line.removesuffix(": test")
        for line in output.splitlines()
        if line.endswith(": test")
    )


def select(group: str, actual: tuple[str, ...], groups=SHARDS) -> tuple[str, ...]:
    """Require a nonempty, disjoint, exhaustive partition; fail closed on drift."""
    if group not in groups:
        raise ValueError(f"unknown shard: {group}")
    if not actual or any(not tests for tests in groups.values()):
        raise ValueError("empty test inventory or shard")
    expected = tuple(test for tests in groups.values() for test in tests)
    for label, names in (("inventory", actual), ("partition", expected)):
        duplicates = sorted(name for name, count in Counter(names).items() if count != 1)
        if duplicates:
            raise ValueError(f"duplicate tests in {label}: {', '.join(duplicates)}")
    missing = sorted(set(actual) - set(expected))
    stale = sorted(set(expected) - set(actual))
    if missing or stale:
        raise ValueError(f"test partition drift: unassigned={missing}; stale={stale}")
    return tuple(groups[group])


def run(group: str) -> int:
    listed = subprocess.run(
        [*CARGO, "--", "--ignored", "--list", "--format", "terse"],
        cwd=ROOT, check=True, text=True, stdout=subprocess.PIPE,
    )
    actual = inventory(listed.stdout)
    tests = select(group, actual)
    print(f"QEMU shard {group}: {len(tests)}/{len(actual)} ignored tests", flush=True)
    for test in tests:
        print(f"  {test}", flush=True)
    # Multiple exact libtest filters are a union. Each test retains every
    # debug/release, Hart, device and watchdog scenario in its Cargo entry point.
    result = subprocess.run(
        [*CARGO, "--", "--ignored", "--exact", "--nocapture", "--test-threads=1", *tests],
        cwd=ROOT, check=False,
    )
    return result.returncode if result.returncode >= 0 else 1


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("group", choices=SHARDS)
    args = parser.parse_args(argv)
    try:
        return run(args.group)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"QEMU shard failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
