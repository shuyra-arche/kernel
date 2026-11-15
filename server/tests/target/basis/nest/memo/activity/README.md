# Authorized CPU activity acceptance

Build the host utility first, then run from the Kernel root:

```sh
cargo +nightly build --locked --manifest-path server/tests/runner/Cargo.toml --target x86_64-unknown-linux-gnu
verify=server/tests/runner/target/x86_64-unknown-linux-gnu/debug/verify
```

the Rust `activity` runner archives the exact Kernel HEAD and pinned Vanllia fixture
`fba906f74cf56b275603c4370b736cde1b0cea20`. It builds matching API/Client/Server,
Capsule 11 and Ledger 11 with the production target flags and the checked-in
`tests/latency/Cargo.lock`. It removes only the fixture's historical ABI
feature and device workload, retaining the CPU memory qualification. No device
or External A/D capability is inferred.

Use Python 3.12+, the root README toolchain, QEMU 10.0.x and OpenSBI with DBCN.
Set `QEMU_RISCV64` and `QEMU_OPENSBI` as in the native guest guide. Run from the
Kernel root with the fixture checked out at that pin:

```sh
"$verify" activity --kernel "$PWD" \
  --fixture /absolute/vanllia --output /tmp/activity-debug --profile debug
"$verify" activity --kernel "$PWD" \
  --fixture /absolute/vanllia --output /tmp/activity-release --rounds 5
"$verify" activity --kernel "$PWD" \
  --fixture /absolute/vanllia --output /tmp/activity-unsupported --unsupported
"$verify" activity --kernel "$PWD" \
  --fixture /absolute/vanllia --output /tmp/activity-stopped --stopped
```

Each command covers 1 and 4 Harts with 128 MiB RAM and multi-thread TCG. A boot
requires every scenario marker, normal Task shutdown, no FAIL/fatal output and
an enforced maximum of eight visits per turn. Four-Hart writer tests also
require trap evidence that the three actual user writer Tasks ran on at least
two Harts. Each boot has a 120-second outer timeout and bounded internal waits.

The supported CPU is explicitly `rv64,svade=off,svadu=off`: legacy hardware A/D
updates with no unknown ADUE policy. The disposable boot provider attests only
this configuration. `--unsupported` uses Svade with no sampling qualification;
Share.Derive must reject before altering the preset bits or publishing grants.
Existing production boot providers default to unqualified.

## Executed boundaries

| Scenario | Evidence |
| --- | --- |
| Actual read/write/idle | Native user volatile loads/stores and successive completed epochs; initial preset A/D is discarded |
| Independent readers | Two recipients, unchanged unread output, captured true retained until each exact acknowledgement |
| Aliases and replacement | Three Memos in two Nests of one Page; only the accessed PTE reports activity; Stale identity never follows a replacement Page |
| Authority | INSPECT alone, out-of-window and forged source Pends reject; a child receives two readonly Sign/View pairs without sampling control |
| Minimal policy consumer | Child maps its own readonly data, reads two streams, sorts candidates in user space and exits with mappings and an unread sample |
| Output withdrawal | Actual user store to readonly data faults; RW publication rejects; cancelled mappings reach Empty and an actual later read faults |
| Capacity/fairness | Sixteen subscriptions receive notifications, seventeenth gets Busy, retained slots become reusable after cancellation |
| Admission rollback | With 31 foreign-sender charges, a two-Pend pair fails atomically and the last ordinary slot remains usable |
| Large geometry | A real level-1 Cell/reservation is Unsupported; it is not split for sampling |
| Continuous writing | Three real user Tasks write the same CPU mapping during repeated clears/fences; dirty evidence survives; writer Hart masks are recorded |
| Partial synchronization | Early-boot real Harts withhold acknowledgements; production Pending and History keep exact facts; a delayed prior acknowledgement cannot finish the next round |
| Observation stops | Real Kernel observation Task exits after one captured clear; normal Retract/Block/Vacate, allocation and output Return still complete |

All diagnostics, timing counters and withholding are test-owned modules under
this directory, attached only inside the archived provider. The complete
composition diff is recorded. Production visibility, features and entry points
are unchanged. The early-boot fence probe supplies captured test bits to the
production history; it proves retry bookkeeping with actual hardware fences,
not hardware A/D generation. The native guest and host CAS race separately
exercise those boundaries.

Each output contains inputs/tool versions/cgroup and affinity, build log,
composition diff, lockfile/ELF/firmware/Capsule hashes, serial/stderr and parsed
results. `TRACE activity-step` records log2-nanosecond bins; bin n represents
`[2^n, 2^(n+1))` ns. Counts include cancellation visits. Asynchronous round
counts also include the boot probe and output retirement. Durations include
preemption and emulator scheduling. Writer throughput is a sequential 500 ms
baseline followed by at least 500 ms and three completed epochs with one active stream in the same boot; order,
TCG, instrumentation and sample period confound causal comparisons. Repeat
and report distributions, never a WCET or a claim that sampling improves speed.

CI retains raw evidence and executable artifacts for supported debug/release,
unqualified Svade and stopped-observer scenarios. Host tests and target builds
have separate claims. This suite does not qualify physical hardware,
Sv39/Sv48, Svadu/ADUE firmware, hotplug, External activity or persistence.

## Interface

No new Kernel call is declared. Use existing Share.Derive with AccordRequest,
a live sampling source and `model::memory::activity::limits(page)`. The existing
return is the receiver-local Share Pend. `Client::memory::activity::pair(view)`
resolves `(Sign, Share View)` using Pend.Inspect/Collect; a foreign recipient
runs that lookup in its own Task. For a nonzero source coordinate, map the
View Pend's actual Pages offset, not an assumed zero.

Reviewed source inputs must match the SHA-256 contracts under
`server/tests/runner/composition/`. An incompatible historical baseline fails
explicitly and requires a separately reviewed composition contract. See the
[verification guide](../../../../../README.md) for evidence and CI ownership.
