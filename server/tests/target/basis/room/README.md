# Native Room synchronization and latency

Build the host utility first, then run from the Kernel root:

```sh
cargo +nightly build --locked --manifest-path server/tests/runner/Cargo.toml --target x86_64-unknown-linux-gnu
verify=server/tests/runner/target/x86_64-unknown-linux-gnu/debug/verify
```

The current ABI 11 suite composes Kernel with Vanllia at
`50ebf0feaae16374ba431f83a515fe6ce273c8b7`. It tests the Scheduler, retained
IRQ/Sign delivery and real-time scheduling contract behind #120. Minimal UART
and real-time correctness cases run in `Guest tests (QEMU)`; repeated performance
comparisons require an explicit request. The historical ABI 10 notes below
remain an audit record, not current native acceptance.

## Synchronization ownership

| Owner / transition | Required synchronization and effects |
| --- | --- |
| Scheduler run/dispatch/steal/remove; Slot enqueue/enter/leave/handoff/step; Sign wake | COMMIT still coordinates Ready membership, Task identity, one running Hart/TrapContext, cancellation, wait/exit, priority, budget and fixed-Hart eligibility. Dequeue and seat are atomic against removal and stealing. An off-queue Ready Task cannot escape that transaction without a checked handoff protocol. No second queue directory is introduced. |
| Scheduling timer | The same slot transaction stages the newest local deadline and retains programming responsibility. Safe Task return merges current heap and retry deadlines, then programs the comparator outside COMMIT. Failed heap admission retains staged work and the existing retry. A local TrapGuard excludes reception callbacks during merge/programming; boot, idle, heap-head changes and IPI refresh remain immediate. |
| Router claim | Only the controller Hart claims, with a local nonrecursive service guard. A controller hardware gate serializes enable-word RMW against peer-Hart revocation. At most four actual masked claims are retained before software publication. This gate is released before acquiring COMMIT. |
| Router publish / retry | Each retained claim moves under COMMIT to its Source's real claim state, including no receiver, or to an owned completion batch for unknown/reserved IRQs. That batch completes after COMMIT. A failed front-half try retains claims, masks external CPU delivery and arms retry. Safe return/idle maintenance joins FIFO COMMIT admission; Scheduler no longer calls the router while owning COMMIT. |
| Source maintenance | Four requested Sources per service turn, each with its own short COMMIT. A 64-bit coalesced request bitmap and round-robin cursor use immutable, never-reused registration slots (maximum 64), distinct from hardware line numbers. Requests made during a turn remain pending. Notifications happen after releasing that Source's transaction. |
| Revoke / Consume / rebind | Live Pend ancestry, full writable View with ASSOCIATE, and unique LISTEN+CONSUME holder remain authoritative. Exact consumption alone completes/rearms; detach keeps physical claim responsibility. Ordinary Source mask and completion remain under COMMIT plus the hardware gate so stale maintenance cannot unmask a revoked/rebound source. PLIC enable-before-complete ordering remains. Pointer/target matches only route maintenance; they never authorize it. |

Source maintenance no longer scans every source on every scheduling turn or
requests work for unrelated Pend targets. Relevant ancestor changes still request
live revalidation. The retained hardware gate/COMMIT order has no cycle: no
hardware gate holder enters COMMIT. No service, queue or hardware guard survives
a context switch. The minimum atomic Source hardware actions are short, but
neither their physical duration nor firmware latency is established by QEMU.

## Attribution

Diagnostics are optional (`kernel/latency`), fixed-capacity and initialized
in place before secondary startup. Reports print actual storage bytes per Hart
(currently 27,136), plus Task-owned observation atomics. Sampling introduces no
allocation, formatting, log I/O or new global lock. Observations never authorize
queue membership or resource access.

| Record | Meaning and limits |
| --- | --- |
| `latency-phase` / `latency-window` | Boot/setup, warmed workload and teardown use explicit boundaries. A span belongs to a phase only when its entire interval fits. Crossing spans stay in aggregate metrics and `latency-crossed`, avoiding contamination of steady distributions. Normal production boot uses the lifetime window; only the archived test composition brackets the warmed workload via existing Debug.Put markers. |
| `latency-phase-outer` / `latency-outer-parts` | Same-transaction timestamp, outer site, inclusive duration and call counts for scheduler step, Sign wait, IRQ maintenance, controller claim, nested SpinLock acquisition and scheduling timer arm, plus Sign deadline redemption, due-heap drain, exact waiter detach, continuation/wake placement and doomed-Task inspection. Eight sites per Hart/phase retain their own largest transaction. Nested durations overlap; never sum them as an exclusive partition. Uninstrumented work remains unknown. |
| `latency-phase-ready` / `latency-ready` | Initial/current queue Hart, destination, enqueue occupancy, queue entries, reselections and rejection bits (ineligible, budget, outranked). Task notification attempt, target and nonwrapping generation are matched to actual SSIP receipt and selection. A receipt can cover coalesced notifications; it does not prove one SBI call caused it. Stealing/uncovered generations report receipt zero (unknown). Queue counts are boundary snapshots, not a continuous runnable competitor census. |
| `latency-phase-irq` / `latency-irq` | IRQ-off origin and transaction active at entry. That transaction can finish within the interval. Reception windows end/restart the interval around actual SIE enablement; the assembly reception handler does not mutate Rust attribution. Firmware, host descheduling and uninstrumented kernel time cannot be separated. |
| `controller-claim` / `timer-return` | Hardware claim interval and deferred timer merge/programming. Leaving COMMIT does not remove these costs from IRQ-off or whole response measurements. |

Per-Hart histograms have 32 log2 buckets, with the last bucket saturated.
Quantiles are bucket upper bounds, not exact order statistics. `probe` is a
calibration floor, not a full overhead bound. Independent phase quantiles or
maxima cannot be added; use paired transaction records for attribution. No
observed maximum is a WCET or deployment response guarantee.

## Workloads and boot policy

- `room`: eight same-Team Tasks each warm up with 64 Yield turns and eight timer sleeps, then execute
  512 timed Yield turns and eight sleeps. Workers stay alive until the main
  Task closes the observation window, separating exit/reclamation costs.
- `irq`: real UART THRE / PLIC line 10; four Yield competitors and six LISTEN-only
  observers; 16 warmup and 256 timed pulses by default. It checks retained and
  repeated delivery, 16 masked burst toggles, stale/double Consume rejection,
  unrelated endpoint isolation, forwarding, driver exit while claimed, Sign,
  View and ancestor View revocation, and four successful rebindings. Separate
  kernel IRQ-to-Sign and guest event-to-consumer histograms must not be added.
- `realtime`: one-Hart admission rejection; on four Harts, genuine User Tasks
  test fixed Hart, budget exhaustion/replenishment, Freeze without credit reset,
  higher-priority Sign wake preemption of pure computation, overload rejection
  and restoration of ordinary scheduling capacity.

These are isolated test compositions. The runner archives the provider and
fixture, records every edit, checks each reviewed patch preimage, retains original/resolved lockfiles, and supplies current provider paths. No native
RAM Publish or DMA is needed; SMP memory qualification stays unavailable.

The IRQ image explicitly gives the initial Team a trusted static UART mapping
using the existing external/MMIO boot path. PLIC controls stay private. View
Pend withdrawal tests the IRQ authority association, not withdrawal of that
lifetime boot mapping. The real-time image loads the ELF into a User Team using
the real TeamBuilder in the archived provider copy; normal Capsule entry is
Supervisor and correctly rejects real-time admission. Neither adaptation weakens
production admission, introduces a permanent test ABI, or proves general native
SMP memory acceptance. Logical Task exit is distinguished from actual View loan
restoration before rebind.

## Run and preserve evidence

Use the root README toolchain, QEMU 10.0.x and `QEMU_OPENSBI`. Run from Kernel:

```sh
"$verify" room --kernel "$PWD" --fixture /path/to/vanllia --output /new/evidence --mode irq --events 16 --rounds 1 --profile debug
"$verify" room --kernel "$PWD" --fixture /path/to/vanllia --output /new/rt-evidence --mode realtime --harts 4 --rounds 1
"$verify" room --kernel "$PWD" --baseline /path/to/control --fixture /path/to/vanllia --output /new/comparison --mode room --harts 1 4 --rounds 5
```

Repeat the comparison with `--mode irq`, and both modes with `--no-latency`.
Both providers build before measured boots; A/B order reverses each round.
Keep the toolchain, firmware, lockfile, quota and affinity fixed, and avoid
concurrent builds or tests. Run all failures/outliers through the same reporting
rules. Diagnostic-on/off elapsed times expose workload perturbation; do not
subtract an assumed constant instrumentation cost.

A published control for #120 is
`ab4dd4cb0a16c257627a6bff3c7f76bf383dcdb6` (`exp/room-control`). It retains the
original IRQ and scheduling-timer COMMIT boundaries with the same diagnostics
and native ASSOCIATE compatibility correction. It is an instrumented control,
not unmodified main. The final PR records candidate revisions, repeated results,
setup failures and remaining unknown causes.

Each output retains exact source/fixture/suite identities, composition diff,
locked build command, compiler/emulator versions, firmware/ELF/image hashes,
quota/affinity, host load, boot order, serial/stderr, statuses and all histograms.
CI uploads minimal correctness evidence as `native-room`. To request the
optional 80-boot comparison in a PR, put these two lines in its body before
pushing an update:

```text
Room comparison: requested
Room baseline: ab4dd4cb0a16c257627a6bff3c7f76bf383dcdb6
```

For first-use analysis, `--cold-room` retains the former Yield-only warmup.
`--compare-room-warmup --mode room --harts 1 --rounds 5` builds cold and warm
images of the same provider before alternating their boot order. Both guests
execute the exact same platform/Kernel ELF, recorded in `boot_platform` and
each result's `platform_sha256`; differing build paths cannot change the kernel
code layout between variants. The timed
512 Yield turns and eight sleeps per worker stay identical. Raw boot/setup
histograms retain first-use work; warming does not remove that cost from the
record. A warmup marker and inputs.json record the selected policy.

The opt-in job also runs this ten-boot study with provider
`5ec49757b92e129822a50e543a999fb4ea509036`, whose tree matches the previous
CI merge exactly. This separates a warmup effect from the newer diagnostic
scopes changing code layout. QEMU first-use translation, cache and firmware
costs are not independently measured.

The opt-in job fetches the validated full control SHA from this repository and
uploads all rounds as `room-comparison`. Its host is separate from a local
comparison: report each environment independently. Raw logs belong in artifacts,
not permanent documentation. No physical hardware timing has been verified.


Reviewed source inputs must match the SHA-256 contracts under
`server/tests/runner/composition/`. An incompatible historical baseline fails
explicitly and requires a separately reviewed composition contract. See the
[verification guide](../../../README.md) for evidence and CI ownership.
