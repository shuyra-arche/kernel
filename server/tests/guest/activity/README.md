# Authorized CPU activity acceptance

`run.py` archives the exact Kernel HEAD and pinned Vanllia fixture
`dd32c35dbb67e2e78edb00ad35f968d16423249e`. It builds matching API/Client/Server,
Capsule 11 and Ledger 11 with the production target flags and the checked-in
`tests/latency/Cargo.lock`. It removes only the fixture's historical ABI
feature and device workload, retaining the CPU memory qualification. No device
or External A/D capability is inferred.

Use Python 3.12+, the root README toolchain, QEMU 10.0.x and OpenSBI with DBCN.
Set `QEMU_RISCV64` and `QEMU_OPENSBI` as in the native guest guide. Run from the
Kernel root with the fixture checked out at that pin:

```sh
python3 server/tests/guest/activity/run.py --kernel "$PWD" \
  --fixture /absolute/vanllia --output /tmp/activity-debug --profile debug
python3 server/tests/guest/activity/run.py --kernel "$PWD" \
  --fixture /absolute/vanllia --output /tmp/activity-release --rounds 5
python3 server/tests/guest/activity/run.py --kernel "$PWD" \
  --fixture /absolute/vanllia --output /tmp/activity-unsupported --unsupported
python3 server/tests/guest/activity/run.py --kernel "$PWD" \
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
Observe must reject before altering the preset bits or publishing grants.
Existing production boot providers default to unqualified.

## Executed boundaries

| Scenario | Evidence |
| --- | --- |
| Actual read/write/idle | Native user volatile loads/stores and successive completed epochs; initial preset A/D is discarded |
| Independent readers | Two recipients, unchanged unread output, captured true retained until each exact acknowledgement |
| Aliases and replacement | Three Memos in two Nests of one Page; only the accessed PTE reports activity; Stale identity never follows a replacement Page |
| Authority | INSPECT alone, out-of-window and forged Reservation reject; a readonly child receives only two selected INSPECT/SAMPLE grants |
| Minimal policy consumer | Child maps its own readonly data, reads two streams, sorts candidates in user space and exits with mappings and an unread sample |
| Output withdrawal | Actual user store to readonly data faults; RW publication rejects; cancelled mappings reach Empty and an actual later read faults |
| Capacity/fairness | Sixteen subscriptions receive notifications, seventeenth gets Busy, retained slots become reusable after cancellation |
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
baseline followed by 500 ms with one active stream in the same boot; order,
TCG, instrumentation and sample period confound causal comparisons. Repeat
and report distributions, never a WCET or a claim that sampling improves speed.

CI retains raw evidence and executable artifacts for supported debug/release,
unqualified Svade and stopped-observer scenarios. Host tests and target builds
have separate claims. This suite does not qualify physical hardware,
Sv39/Sv48, Svadu/ADUE firmware, hotplug, External activity or persistence.
