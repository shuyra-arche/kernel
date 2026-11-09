# Bounded latency: M1 baseline and M2 progress

This records the M1 baseline and the first M2 admission boundary for #30. No current operation has a hard real-time guarantee.
The existing FIFO/time-slice scheduler, authorization and retirement rules remain
unchanged. QEMU observations are neither WCET bounds nor admission proofs.

## Operation contract

| Class | Operations | Required preparation / work parameters | Waiting or exclusion |
| --- | --- | --- | --- |
| Candidate for a future bounded path | Clock read; retained Sign poll; Sign consumption receipt; already-created Task handoff | Resident code/stack/data; fixed admitted Harts H; no first-use extension allocation; one exclusive IRQ processor; fixed routing graph | Poll/Consume still enter COMMIT; scheduler searches Harts/ready queues; no current blocking-time bound |
| Asynchronous completion only | hardware source completion after Consume; Sign Listen; deadlines; delegated page fault; external Memo retract; Nest close | Precreated resources, explicit deadline and failure policy; source lifetime remains valid | Caller blocks; readiness is distinct from execution; IOMMU completion and safe reclamation are distinct from business/IRQ completion |
| Forbidden synchronously in a future deadline-critical section | Allocation/Cell reshape; Task/Team creation; arbitrary Clone copy; arbitrary Pend subtree revoke; Unit teardown; bulk routing/cleanup; debug output; firmware reset | Perform before admission or after leaving the critical path; no implicit fallback from a prepared operation | Allocator search/zeroing, graph traversal, firmware calls, remote TLB/IOMMU progress and reclamation have unproved/unbounded duration |

The first row is an analysis candidate, not an exported RT service class. Blocking
completion must not be reported as bounded simply because one polling iteration
is bounded. Sign notification coalesces work; it is not a device completion count.

## Boundary audit and smallest repair order

1. **COMMIT / RelLock / SpinLock** (`basis/commit.rs`, `metal/riscv/sync/`):
   interrupts are masked before acquisition. Contended acquisition loops until a
   peer releases; there is no fairness or acquisition bound. RelLock recursion
   extends the outer holder's lifetime. Debug lockdep detects invalid nesting but
   proves no time bound. First catalogue and shorten long holders without moving
   authorization checks away from atomic publication; then introduce narrower
   transaction boundaries. External IRQ admission and deadline draining now try
   COMMIT once and retain deferred work on failure; ordinary lock callers and
   successful callbacks remain outside any time-bound claim. Replacing the lock
   type alone is insufficient.
2. **Sign waiting and routing** (`basis/sign/wait/`, `endpoint/`): fan-out depends
   on forwarding edges F, wake chains on waiters W, removal searches a chain,
   and space cleanup scans sites S. Task cancellation uses its authoritative
   wait key and scans only that one chain, rather than every shard/site. The timer batch is at most 64 tickets,
   but each ticket may search W and launch through H schedulers. Bound one batch
   and retain a continuation before splitting the commit region. Hardware IRQs
   have one processing branch, at most 64 sources and four claims per entry.
   Retained publication is separate from deferred notification: each key wakes
   at most four waiters and has at most eight forwarding destinations per batch,
   with continuation retained. Callback cost, repeated batches, the shared commit
   lock and scheduler still defeat a latency proof.
3. **Scheduler** (`basis/room/scheduler/`, including `slot/`): admission, steal and
   compatible-Hart selection depend on H and queued Tasks Q; extended-state
   save/restore depends on F/D/V state and VLEN. There is no priority, affinity or
   budget admission contract. Preserve this baseline before M3 policy changes.
4. **Lifecycle and Pend** (`basis/unit/lifecycle/{doom,reap}.rs`, `basis/pend/`):
   descendant walks, ledger cleanup and repeated husk processing depend on live
   objects and graph size. A burial turn processes at most four Tasks, but each
   Task retirement can still traverse descendants. Remaining husks retain a local
   timer retry instead of depending on another Task to yield. Logical closure can be bounded separately from
   asynchronous physical cleanup; preserve generations and immutable exit results.
5. **Cell/Nest and hardware retirement** (`basis/cell/`, `basis/nest/`,
   `metal/riscv/iommu/`): zeroing depends on bytes; traversal on mappings/pages;
   allocator operations depend on free state. Remote invalidation and device
   completion require progress outside the caller. Keep retirement tickets and
   retained ownership until trusted completion; never turn timeout into reuse.
6. **Trap/firmware** (`entry/trap/`, `metal/riscv/sbi/`): trap entry/return,
   extension handling, MMIO, SBI timer/IPI/console, cache/translation misses,
   interrupt storms and firmware interference have no demonstrated hardware
   upper bound. QEMU only supplies functional coverage and a reproducible load.

The first remediation priority is long COMMIT holders plus IRQ-off acquisition:
all otherwise short candidate paths inherit their interference. M1 adds no
priority, budget, lock replacement or asynchronous reclamation shortcut.

## Measurement definitions

Enable provider feature `kernel/latency` on the trusted platform image. Default
images contain neither hooks nor measurement storage. Boot allocates one fixed
metric block per admitted Hart before secondaries run. Sampling uses a fixed
number of relaxed atomic operations and `time` CSR reads: no allocation, lock,
retry loop, console write or graph walk. Histograms have 32 fixed bins: 0 ticks
in bin 0, [2^(i-1), 2^i-1] in bin i, with bin 31 including all larger values.
Count and exact observed maximum accompany each histogram. Counters wrap after
2^64 samples; finite baseline runs must stay below that limit.

| Metric | Exact boundaries / limitation |
| --- | --- |
| irq-off | Explicit TrapGuard disable-to-drop when SIE was enabled, plus active Rust trap/idle-service spans which already enter with SIE clear; excludes assembly entry/return and WFI idle |
| kernel-active | Rust trap entry to return preparation, split around WFI; includes contention and nested operations, excludes idle waiting and assembly |
| spin-wait / spin-hold | After IRQ mask to successful acquire (including debug lockdep), and guard construction to drop before unlock; failed try_lock is not a successful sample |
| commit-wait / commit-hold | RelLock acquisition/guard scopes (COMMIT is its only current instance); nested scopes are included separately, so sums are not CPU utilization |
| irq-sign | External router entry before COMMIT acquisition to successful retained driver Sign publication; excludes deferred waiter wakeup, device-to-CPU and earlier trap prologue; reserved IOMMU queues and unpublished/cancelled delivery are not samples |
| ready-run | First ready-queue publication through the final Rust user-return boundary after extension restore and ASID occupancy; preserved across queue transfer/steal, cleared when parked; excludes only the final assembly return |
| timer-late | Deadline drain observation minus requested tick, using the existing #27 drain boundary; not time of user execution |
| probe | 1024 boot-Hart samples of two time reads and one same-layout histogram update; calibration floor, not an overhead subtraction or hardware upper bound |

Metrics are observations at named boundaries, not disjoint intervals. Sampling
perturbs execution and lock contention; per-Hart storage avoids a shared sample
lock but does not eliminate cache/coherence cost. Reporting disables sampling
only after all Harts reach shutdown. Firmware output occurs outside the sampled
workload. Per-Hart empty metrics are valid; the guest validator requires every
metric to be exercised across the run.

## Reproduction and evidence

Use the pinned Vanllia fixtures from `.github/workflows/check.yml`, including its
local provider patch. Run:

```
cargo test --locked --workspace --target x86_64-unknown-linux-gnu
cargo build --locked --workspace --features kernel/latency --target riscv64gc-unknown-none-elf
cargo build --locked --workspace --features kernel/latency --release --target riscv64gc-unknown-none-elf
cargo test --locked -p kernel-memory-tests --test hardware --target x86_64-unknown-linux-gnu latency_baseline_observes_contended_paths -- --ignored --nocapture
```

The baseline uses real UART THRE delivery with four runnable yield workers,
blocking observers, deadline waits, revocation and driver replacement, followed
by the existing Cell/Pend/Sign lifecycle workload. Debug runs use 1/4 Harts;
release uses 4. The ordinary IRQ fixture exercises identical guest code without
instrumentation. QEMU execution checkpoints prove multiple-Hart reception.
Evidence under `server/tests/guest/cell/target/evidence/latency/` includes commands,
QEMU version, image hashes, serial output and per-Hart count/max/histogram TSV.
The firmware is OpenSBI supplied by the recorded QEMU package; timebase comes
from the admitted DTB and is printed as `ticks-hz`. Run on isolated physical
hardware with fixed firmware/cache/frequency and interference assumptions before
using measurements for hardware timing analysis. The rest of M2, M3 and hardware WCET proof remain open.

## M2: retained retry at interrupt admission

External IRQ handling uses one `RelLock::try_lock` attempt before touching PLIC
claim state, source bindings or Sign receipts. Foreign ownership returns
immediately from that attempt: the controller keeps the unclaimed interrupt,
local SEIE is disabled, and a Hart-local retry deadline is retained. The optional
IRQ→Sign metric keeps the first entry timestamp across deferrals, so retry delay
is included rather than hidden by starting a fresh sample. Rearming
also tries admission once; only successful maintenance reenables deferred local
delivery. The existing claim/unmask/complete ordering stays under COMMIT.
Source death/revocation is revalidated by the existing binding state machine.

The front half no longer performs arbitrary external-Nest retirement. Every
successful claim schedules maintenance even if the Hart would otherwise disarm
its comparator. A 100-microsecond retry interval is a backoff choice, not a
response-time promise. Earlier future comparator deadlines are preserved; an
elapsed comparator is advanced to avoid a level-triggered interrupt storm.
Ordinary timer refresh takes the minimum with the retained retry. A failed heap
try-lock leaves both registrations and local scheduling requests intact.
Timer expiry consumes retry state only when due, before retrying maintenance;
failed COMMIT admission drains no tickets or Holders.

`RelLock::try_lock` retains same-Hart recursion and IRQ-state restoration. It
never temporarily enables SIE: current kernel trap entry uses a shared per-Hart
stack and is not a general nested/preemptible kernel entry. Host tests execute
the production lock atomics and guards with a Hart/IRQ boundary fixture; the
foreign-owner handshake requires rejection before releasing the other owner.
Production retry/comparator tests cover earlier deadlines, repeated contention,
expired settings and failed comparator writes. Actual hardware masking and
wakeups require the IRQ/IOMMU, clock and lifecycle QEMU scenarios.

This is **not completion of M2**. Successful source callbacks, Timer/idle
resource maintenance, scheduler COMMIT acquisition, other SpinLock/RelLock
callers, one-site waiter searches and recursive Pend/Task retirement still need
narrower ownership and bounded continuations. Under permanent contention,
deferred work can remain pending indefinitely. No hard-real-time service class
or hardware latency bound is introduced by this change.
