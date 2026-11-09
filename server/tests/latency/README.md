# Bounded latency: M1 baseline

This is the M1 audit for #30. No current operation has a hard real-time guarantee.
The existing FIFO/time-slice scheduler, authorization and retirement rules remain
unchanged. QEMU observations are neither WCET bounds nor admission proofs.

## Operation contract

| Class | Operations | Required preparation / work parameters | Waiting or exclusion |
| --- | --- | --- | --- |
| Candidate for a future bounded path | Clock read; retained Sign poll; hardware source acknowledgement; already-created Task handoff | Resident code/stack/data; fixed admitted Harts H; no first-use extension allocation; one exclusive IRQ listener; fixed routing graph | Poll/ack still enter COMMIT; scheduler searches Harts/ready queues; no current blocking-time bound |
| Asynchronous completion only | Sign Listen; deadlines; delegated page fault; external Memo retract; Nest close | Precreated resources, explicit deadline and failure policy; source lifetime remains valid | Caller blocks; readiness is distinct from execution; IOMMU completion and safe reclamation are distinct from business/IRQ completion |
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
   transaction boundaries. Replacing the lock type alone is insufficient.
2. **Sign waiting and routing** (`basis/sign/wait/`, `endpoint/`): fan-out depends
   on forwarding edges F, wake chains on waiters W, removal searches a chain,
   and space/task cleanup scans sites S. The timer batch is at most 64 tickets,
   but each ticket may search W and launch through H schedulers. Bound one batch
   and retain a continuation before splitting the commit region. Hardware IRQs
   have one bound waiter, no forwarding, at most 64 sources and four claims per
   entry; the shared commit lock and scheduler still defeat a latency proof.
3. **Scheduler** (`basis/room/conductor/`, `scheduler/`): admission, steal and
   compatible-Hart selection depend on H and queued Tasks Q; extended-state
   save/restore depends on F/D/V state and VLEN. There is no priority, affinity or
   budget admission contract. Preserve this baseline before M3 policy changes.
4. **Lifecycle and Pend** (`basis/unit/lifecycle/{doom,reap}.rs`, `basis/pend/`):
   descendant walks, ledger cleanup and repeated husk processing depend on live
   objects and graph size. Logical closure can be bounded separately from
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
| irq-sign | External router entry before COMMIT acquisition to retained driver Sign publication/wakeup; excludes device-to-CPU and earlier trap prologue; reserved IOMMU queues are not driver Sign samples |
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
using measurements for hardware timing analysis. M2/M3 and hardware WCET proof
remain open.
