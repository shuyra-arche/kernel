# Bounded latency: M1 evidence and M2 execution boundaries

M2 supplies interrupt reception during foreign-owner waits, local publication
boundaries and retained cleanup batches for #30. It does not introduce a hard
real-time service class. [M3](../realtime/README.md) adds the conditional
priority/preemption, budget, affinity and Sign service contract. This file
retains the M1/M2 baseline and exclusion audit. QEMU measurements are observations,
not hardware WCET bounds or schedulability proofs.

## Operation contract

| Class | Operations | Preconditions and cost parameters |
| --- | --- | --- |
| Candidate for M3 admission | Clock read; retained Sign observation/consumption; prepared Task handoff | Resident code/stack/data; admitted Hart count H; no first-use extension allocation; bounded authority depth, route fan-out and wait-site occupancy |
| Asynchronous completion | Sign Listen/deadlines; external Memo withdrawal; Pend/Task/Team/Nest retirement | Keep the original identity, completion receipt and ownership across Busy; readiness, execution, device completion and safe reuse are distinct |
| Forbidden synchronously in a deadline-critical section | Allocation/Cell reshape, Task/Team creation, arbitrary Clone copy, bulk revocation/teardown, debug output, firmware reset | Prepare before admission or advance a retained continuation outside the critical section; no implicit fallback to allocation, faults or synchronous remote completion |

No existing operation exports a deadline guarantee. A bounded batch is not a
bound on the number of retries, lock acquisition time, hardware completion or
Task dispatch delay. Ordinary memory calls retain their non-RT contract.

## Exclusion and reception

`SpinLock`, `RelLock` and OnceLock publication waits visit
`metal/riscv/interrupt/window` on every unsuccessful wait iteration. A failed
SpinLock CAS also visits the window. A peer performing a CPU fence is waited on
through the same reception mechanism. Boot does not arm windows until the
Hart's timer, interrupt mask and ROOT execution environment exist; boot is not
an admitted runtime section.

The temporary, aligned direct vector uses no stack, Rust call, allocation,
lock, controller claim or Task switch. It records enabled pending bits with one
atomic OR, masks `sie`, and returns. The caller saves/restores `sstatus`, `stvec`,
`sepc`, `scause`, `stval` and `sie`; every scratch GPR is an explicit assembly
clobber. `sscratch`, ROOT stack ownership and the interrupted continuation stay
unchanged. Code/data used by the vector must be resident, mapped, naturally
aligned and accessible in supervisor mode. The platform must permit the same
S-mode CSR operations as normal trap entry. Synchronous faults in these trusted
instructions are not a recovery mechanism.

The [RISC-V supervisor specification](https://docs.riscv.org/reference/isa/priv/supervisor.html)
requires interrupt conditions to be reevaluated after writes to the relevant
CSRs. This supports a reception opportunity at the explicit SIE write; it does
not provide a cycle bound for firmware, memory or the complete service path.

Every exclusion guard increments a Hart-local depth. Its drop releases the
actual lock before decrementing depth. At depth zero a retained external IRQ
may attempt the existing bounded router front half; a serving flag prevents
recursive callbacks. Foreign COMMIT ownership is tried once, leaving the
controller assertion and retained retry intact on rejection. Timer and IPI
levels are **not acknowledged or reprogrammed by the window callback**. Their
ordinary scheduler entry remains responsible for consuming them. An earlier
implementation reproduced a self-sustaining timer storm in the IRQ/IOMMU guest;
the callback must not manufacture a new timer for every observed timer.

SpinLock acquisition is still not fair or time bounded. COMMIT uses FIFO turns
as detailed below, without a transaction-duration bound. Interrupt *reception* can
occur while the caller waits; full handler work and Task scheduling have their
own admission boundaries. Ordinary trap entry reuses ROOT and is not a generic
nested kernel entry. A kernel continuation is switched only through existing
handoff/return/idle paths. No SpinLock guard, ledger registration or partially
published Task state crosses a Task scheduling point. Same-Hart RelLock
recursion remains one owner; SpinLock recursion remains invalid. Debug lockdep
continues to validate nesting; release builds retain ownership/exclusion.

## Lock and work audit

H is the admitted Hart count (bounded by MAX_HART_SLOTS). D is authority depth;
T is Team ancestry depth. P/R are the admitted Pages/active ranges in one Span,
and N is the admitted page-table/occupancy size of its Nest. P/R/N are memory
operation parameters, not newly promised constant-time backend operations.

| Owner / path | One service attempt and dependency boundary |
| --- | --- |
| Global COMMIT | Owns atomic Task container/state and cross-object authority publication. IRQ admission and timer redemption try once. Retirement CPU withdrawal/fences and retained Task retirement execute outside this guard; no whole Pend graph lock set is acquired. Other acquisition waits receive interrupts through the leaf window. |
| Ledger transaction locks | At most the leaf and immediate parent, ordered by Task ID and tried once. Publication revalidates under COMMIT after fallible preparation. Removal settles the borrow before exposing a childless parent. |
| Pend ancestry | At most 64 nodes; immutable links are attached before publication. Logical root closure invalidates unvisited descendants. One Task ledger gate closes all its hosted authorities. Physical borrowing/routing obligations remain separate until settlement. |
| Pend retirement | 8 postorder leaves per cull, each selected through at most 64 ancestry levels. Return/Revoke make at most 8 attempts (64 leaves total); Busy retains the root. A husk makes one cull attempt. No unused descendant-count accumulator or optimistic graph-version snapshot remains. |
| Ledger/control/source storage | At most 1024 Pend entries, outgoing ledger edges, Hold references, administrative entries, Share authority records, Share mapping records and source bindings in each respective owner. Exhaustion rejects preparation before publication. |
| Task queues | Known-member cancellation/removal is O(1), with intrusive previous/next links and a non-reused queue identity. A moved Drain keeps the old identity; a reused Site gets a fresh one. Scheduler selection examines at most four compatible candidates, each through at most four queue entries; exhausted selection retains a timer retry. Cross-Hart work is bounded by H. |
| Wait sites | 16 shards, at most 256 Sites per shard and 64 waiters per Site; forwarding fan-out at most 8; an Endpoint admits at most 1024 routes with four keys per route. Admission precedes ticket/Task handoff. Ordinary one-site bulk wake is at most 64 waiters; retained IRQ notification remains four waiters per key per batch. ASID cleanup removes at most four Sites per call, retaining the ASID until complete. |
| Timer expiry | At most 64 due tickets. Indexed heap removal and O(1) known-Task unlink preserve one completion per ticket. Failed COMMIT admission consumes neither ticket nor Holder. |
| External IRQ | At most four claims; at most 64 configured sources. Retained publication, waiter notification, consumption receipt and controller completion remain separate. Original first-entry measurement survives deferred admission. |
| Team termination | One root phase closes subtree admission. An intrusive Team-owned queue advances four member/child edges per service turn, without allocating queue storage. Members/children are bounded by 1024 per node, ancestry by 32. Outstanding queued/running sweeps prevent shutdown. Finish closes root admission directly; descendant admission checks ancestry. |
| Task retirement | Four husks per burial turn; one child or one local phase per husk. `Heirs → Pends → Controls → Memory → Receipts → Settled`. Receipt clearance advances one member/child/ancestor edge, preserves other observers' results and tolerates earlier identity removal. Task Drop no longer recursively clears the tree. |
| Share/Hold/control cleanup | Four Share maps, four source bindings, four Hold releases or one caller-controlled mapping per corresponding attempt. Hardware failure retains the exact record. Source-wide closure is published before withdrawal. |
| CPU Nest close | `Open → Draining → Closing → Closed`: one indexed Span per close attempt, no whole-Nest snapshot. New translation admission rejects Draining. A completed intermediate fence leaves Draining unchanged; Closed requires the last Span and its covering fence. Four settled tickets, cleanup nodes or mapping registrations per associated pass. ASID release waits for close, wait cleanup and registration settlement. |
| Translation/allocator locks | One Span's translation work depends on P/R/N and admitted Sv geometry; allocation and rehash depend on admitted storage. These remain non-RT operations. Peer-fence and lock waits visit reception windows. Frame zeroing releases its allocator lock first and visits a window every 64 KiB, before publishing any initialized Cell. |

The work limits constrain software graph/container work. MMIO, cache/coherence,
TLB shootdown, SBI timer/IPI/RFENCE, IOMMU steps, F/D/V save/restore and physical
memory service need platform-specific worst-case assumptions. No timeout,
partial batch or callback turns an unconfirmed Cell into reusable storage.
Large Span/backend work remains outside the candidate synchronous RT set.

## Compatibility and retained progress

ABI layouts and call numbers are unchanged. Large Return/Revoke, Share Delete
and Team/Nest Delete may now return their existing Busy result after partial
cleanup. Retry the **same identity**; do not grant new authority or reconstruct
an object. Logical closure persists across retries. Final success alone means
the required cleanup completed. Metadata capacity/depth exhaustion is rejected
through the existing failure variants before Task/authority publication.

Sign event state remains in the endpoint independently of notification Site
capacity. Rejecting new wait admission does not consume a retained event or
remove any admitted waiter. Existing pending Site notification survives a
failed attempt to create another Site. Hardware source completion is still
validated against its consumption sequence and live binding.

## Measurement definitions

### COMMIT admission order

COMMIT's reentrant lock reserves one FIFO turn per waiting Hart. The outermost
release publishes protected writes and advances that turn; recursive guards
reuse their Hart's current turn. A nonblocking try succeeds only with an empty
queue (or for the existing owner). Failed tries reserve nothing, including the
interval after an owner releases and before the next waiter enters.

With H participating Harts, an outer reservation has at most H−1 earlier turns.
New requests cannot overtake it. This removes starvation from repeated lock
reacquisition; it is not priority inheritance or a bound on transaction time.
Its elapsed wait still includes those preceding outer transactions, any nested
work, firmware and host/hardware scheduling. A stalled predecessor stalls its
successors. Waiters retain the stackless reception windows; no Task switch,
allocation or lock acquisition is introduced in those windows.

Host tests hold waiters at acknowledged reception boundaries to check FIFO
ordering, protected-write visibility, recursion while queued, non-barging tries
and the absence of abandoned turns. The QEMU latency workload continues to use
four competing Yield workers plus real UART IRQ publication and revocation;
functional assertions and watchdogs are unchanged. Compare observed distributions
under the same QEMU mode, but do not infer a hardware upper bound from a lower
maximum in one run.

Enable provider feature `kernel/latency`. Fixed per-Hart histograms use 32 bins,
relaxed atomic updates and the admitted `time` clock. The probe measures 1024
pairs of clock reads plus same-layout updates. Counters must not wrap during an
evidence run. Sampling perturbs execution; it is not an overhead subtraction.

| Metric | Boundaries / limitation |
| --- | --- |
| irq-off | Explicit disable intervals and Rust trap/idle active intervals, split at reception windows; excludes assembly entry/return and WFI. A window ends an IRQ-mask sample, not a kernel-continuation sample. |
| kernel-active | Rust trap entry to return preparation, split around WFI; includes contention and reception windows. |
| spin-wait / spin-hold | IRQ mask to successful acquisition, and guard construction to unlock; failed try is not a successful sample. |
| commit-wait / commit-hold | RelLock scopes, including recursive scopes; sums are not CPU utilization. |
| irq-sign | First router attempt through successful retained publication, including deferred admission; excludes deferred waiter notification and device-to-CPU latency. |
| ready-run | First ready publication through extension restore/ASID occupancy at Rust user return; preserves the timestamp across queue transfer. |
| timer-late | Deadline drain observation minus requested tick, not time of user execution. |
| probe | Measurement floor under the recorded environment, not hardware WCET. |

`latency-windows` reports opens and windows observing retained pending bits.
`captured` is not a count of distinct device events: a level may remain pending
across several windows. Keep kernel-active and deferred IRQ latency alongside
irq-off; otherwise splitting mask intervals could conceal long continuations.

## Verification and reproduction

Use the pinned Vanllia fixture in `.github/workflows/check.yml` with its local
Kernel provider patch:

```
cargo test --locked --workspace --target x86_64-unknown-linux-gnu
cargo build --locked --workspace --target riscv64gc-unknown-none-elf
cargo build --locked --workspace --release --target riscv64gc-unknown-none-elf
cargo test --locked -p kernel-memory-tests --test hardware --target x86_64-unknown-linux-gnu latency_baseline_observes_contended_paths -- --ignored --nocapture
```

Host tests cover real authority ancestry/gates, intrusive queue ownership,
Site admission/notification retention, receipt-clearance cursors, ticket batch
settlement and foreign-owner lock rejection/reception handshakes. Boundary
fixtures do not prove RISC-V CSR masking, controller completion or timing.

The common RV64 guest now creates 80 child authorities: the first Return is
Busy, an unvisited child is already Retiring and cannot Listen, a sibling still
works, and same-identity retries settle the branch. Team deletion retries Busy
with a watchdog while preserving all prior exit/receipt assertions. CPU/MMU,
I/O creator death and UART/IOMMU coexistence cover actual publication, stale
access denial, cascade and hardware retention. The instrumented baseline runs
debug 1/4-Hart and release 4-Hart boots and records reception-window evidence.

Evidence under `server/tests/guest/cell/target/evidence/` contains commands,
versions, hashes, serial logs, watchdog elapsed times and latency TSVs. M1's
historical baseline is linked from #30 / PR #100. Compare only named boundaries
under recorded environments; changed workloads and mask-window boundaries are
not direct hardware performance comparisons. Full CI validates the final commit;
M3 and physical-hardware WCET analysis remain separate work.

## Outer transactions and execution phases

The optional provider now adds `outer-wait` and `outer-hold`: each outer COMMIT
turn contributes once, regardless of recursive guard count. The original
`commit-wait`/`commit-hold` metrics retain their original scope semantics. Up to
eight slowest distinct outer call sites per Hart are retained without allocating,
locking or formatting during a sample. Each `latency-outer` record pairs the
largest observed hold at that site with the wait for that same transaction;
it is a bounded selection, not a complete per-site profile. Attribution uses
static caller locations forwarded through `commit`/`try_commit`. Reporting reads
those Hart-local records only after all Harts reach the shutdown barrier.

`ready-queue` runs from first ready-queue publication to final seating;
`dispatch-run` runs from that seating through extension restoration and ASID
occupancy at the Rust return boundary. Both use the same endpoint and partition
`ready-run` for each completed sample. Transfer preserves the original ready
stamp, reseating replaces the dispatch boundary, and parking cancels the sample.
These do not measure notification work before ready publication, nor final
assembly return instructions. Histogram percentiles cannot be added together.

Initial attribution identified Cell Split/Merge (`cell/object::reshape`) and
Share Open (`flat/sharing::open`) as long outer holders. Split/Merge now retains
its owner's ledger while preparing and converting its exclusive private source
outside COMMIT. Admission requires one Hold, no bindings and no in-flight
publication; the owner ledger prevents a second access and retirement. The
replacement identity is reserved before receipt movement, so successful movement
cannot be followed by a fallible publication allocation. Final publication closes
the old identity; if Stop closed the owner meanwhile, replacement receipts remain
in its ledger for retirement and caller success is refused.

This removes page-count-dependent physical conversion from global COMMIT, but
not from the private Flat/allocator locks. Share Open's publication transaction,
private allocator costs and firmware remain unresolved blocking sources. No
hardware time bound or completion of #30's strongest acceptance is claimed.

Ordinary wake placement also owns its cursor in Task's scheduling state. A shared
global cursor allowed periodic workers to remain on one residue when unrelated
wakeups advanced it by the Hart count. Each Task now progresses independently;
existing compatibility, idle preference and fixed-Hart reservation rules remain.
The context guest still requires actual traced migration; its assertions and
watchdogs are unchanged. IRQ observers park with WaitValue during construction;
their event deadlines start after all observers are prepared. The original four
Yield workers remain the contention load. Common Sign revocation keeps the
recipient alive until exact-identity Busy retries settle; logical invalidation
may wake it before physical ledger removal, and immediate exit would race that
removal. Errors other than Busy still fail the test.
