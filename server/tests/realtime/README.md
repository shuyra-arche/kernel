# Conditional real-time execution — Kernel #30 M3

This is an optional, fixed-priority CPU reservation for prepared User Tasks.
Admission establishes a software scheduling contract under explicit platform
assumptions. It does not establish a physical WCET, an arbitrary Sign service's
completion time, or a hard-real-time guarantee on QEMU.

## Ownership and publication

Task owns `Ordinary → Preparing → Fixed` scheduling state, its budget account,
base/effective priority and one outgoing service dependency. Preparation failure
returns to Ordinary; a successfully published Fixed contract is immutable.
The scheduler retains weak admission registrations, not copies of live budgets.
`Preparing` leases a fully stopped Task, preventing Resume while resident pages
and extension images are prepared **outside COMMIT**. Publication revalidates
live administrative authority and Task state under COMMIT. Stop can cancel it.
The preparation lease also retains retirement: Stop closes live authority
immediately, but resource retirement waits until preparation releases its lease.
An Arc to Task alone is not proof that its context page remains resident.

An administrator with Build qualification and control of the Task calls
`Unit.Admit`. The Task must be initially held, or individually frozen and fully
off-Hart after ordinary preparation. It must own no child Teams and run in User
mode. Supervisor Tasks can mask interrupts and therefore cannot be admitted.
`Unit.Evict` is allowed only before the initial Resume; freezing a used contract
cannot reset or replace its budget. Exit removes the reservation.

## CPU contract

| Field / boundary | Contract |
| --- | --- |
| Hart | Dense boot topology slot; slot 0 is housekeeping and cannot be reserved. The chosen Hart must have the same F/D/V capability as slot 0, retaining an escape destination for displaced ordinary Tasks. Single-Hart systems reject admission. |
| Placement | A Fixed Task stays on its selected Hart; stealing/migration never moves it. Ordinary Tasks vacate that Hart and remain on eligible housekeeping Harts. Resume returns Busy while a previous ordinary continuation still occupies the reserved Hart. |
| Priority | 1–31, higher wins. Declared inheritance ceiling is at least base and at most 31. Select the highest eligible priority; equal priorities retain FIFO rotation on Yield or scheduling ticks. |
| Capacity | At most 32 admitted Tasks per Hart; no allocation in ready/pending membership. |
| Budget Q | 100 μs minimum, strictly below period T. Nanoseconds convert down to hardware ticks; period converts up. Zero-tick budgets fail admission. |
| Period T | 1 ms–10 s. First dispatch starts a window. Blocking, Yield and freeze preserve the account. Unused credit does not accumulate. |
| Exhaustion | Keep the Task ready but ineligible until replenishment. Delayed timers catch up in constant arithmetic work. Dispatch never lends another Task's credit. |
| Overrun | Charge actual elapsed ticks, including the Task's kernel work. Record time beyond the available budget; delayed reception does not hide excess execution or manufacture credit. |
| Observation | `Unit.Profile` returns the contract, effective priority, remaining budget, absolute next replenishment, cumulative consumed/overrun time and conditional response calculation. Dormant replenishment is zero. |

Every outer Task return checks ready priorities, placement and remaining budget.
Timer/IPI and syscall return use the existing ROOT handoff boundaries. No Task
switch occurs in M2's stackless reception vector or across a live exclusion
operation. Ordinary syscall returns preserve the existing maintenance deadline.
A remote Sign publication nudges reserved Harts; the receiving Hart rechecks
its retained local waiters and may preempt a lower-priority computation.

## Sign services and cancellation

`Sign.Assign` binds a direct endpoint to a controlled admitted server and its
live Trigger grant. ROUTE authority is required. `Sign.Resign` removes that
binding. The retained binding uses weak references to the original live grant
records; it neither copies authority nor forms an Endpoint/Pend reference cycle.
Rebinding creates a distinct identity. An old wait cannot inherit a new binding.

`Sign.Await` explicitly declares a synchronous service dependency. Ordinary
`Listen` remains an independent observation. Caller and server must be admitted
on the same Hart. A Task has one outgoing edge; the per-Hart graph is bounded by
32 nodes and eight edges per chain. Cycles, self-waits, cross-Hart edges and
inheritance above an admitted ceiling fail before edge publication. Effective
priority is the maximum base priority reaching a node. Recompute it after
completion, timeout, caller cancellation, server exit, grant revocation or
binding removal, preserving other callers' donations.

Only priority is inherited. A server with exhausted credit waits for its own
replenishment, even if a high-priority caller waits. A blocked server needs its
own service/arrival bound for an end-to-end deadline. Await does not promise
that a server will answer, nor does a timeout cancel the server's work.

Events retain the existing one-pending-trigger and sequence/Consume semantics.
Admitted producers publish first and enqueue allocation-free notification work;
housekeeping advances four endpoints per turn using existing bounded listener
batches. Admitted consumers are retained in the Hart's pre-existing intrusive
queue (at most 32), without global Site, forwarding edge, ticket or timer-heap
allocation. SleepUntil uses the same local continuation storage and absolute
Clock epoch. A completed local wait removes its dependency exactly once before
becoming Ready. Invalid grants/bindings complete with failure; cancellation
never consumes an event. Dead servers report Dead; revoked grants report Denied.

## Allowed work and resource failure

| Admitted call | Conditions |
| --- | --- |
| Room Clock / ClockInfo / Yield / SleepUntil | Existing monotonic clock, local budget and local deadline continuation. |
| Sign Listen / Await / Trigger / Consume | Preconstructed live grants. Listen/Await accept direct endpoints only; arbitrary routed-source scanning is rejected. Hardware Sign ownership still requires its established `(View, Sign)` binding. |
| Unit Exit | Preserve result/reason. Diagnostic note copying/output is omitted. Execution ends immediately; logical hooks and physical retirement run on housekeeping Harts. |
| Other operations | Denied before domain effects, including creation, mapping changes, Clone/Share transfer, Pend mutation, WaitValue/WakeValue and Debug. |

Admission materializes the current instruction page and the Kernel-owned stack
(up to 64 KiB), and prepares every declared F/D/V save area. A previously enabled
extension must be included and compatible. The controller must prepare **all**
other code/data and grants before activation, and keep them valid for the work.
There is no implicit global pin or promise that mutable mappings cannot be
revoked. An admitted page fault, undeclared extension first use, or inaccessible
syscall instruction terminates the Task instead of allocating, delegating a
fault, or synchronously waiting for a remote page provider.

Doomed Tasks retain their existing intrusive membership in a terminal queue.
Housekeeping advances four logical exits per turn, then the M2 phased husk
retirement. Logical death closes live authority; completion of Task execution
is distinct from Cell reuse. Pend withdrawal, CPU shootdown, IOTINVAL/IOFENCE
and hardware settlement retain their original identity and asynchronous
obligations. No successful timeout or accounting transition bypasses them.

## Conditional response analysis

For a continuously runnable request needing at most Q_i execution and having
credit, the implementation iterates this conservative sufficient test:

```
R_i = Q_i + B_i + Σ[j != i, ceiling_j >= priority_i] Q_j × (ceil(R_i / T_j) + 1)
```

The extra budget covers a competing fixed-window server's carry-in. Competitors
are counted at their maximum possible inherited priority; the subject is counted
at its base, so inheritance cannot invalidate an accepted CPU interference set.
All existing reservations are checked again before adding one. Overflow,
nonconvergence after 256 iterations, capacity failure or any R_i > T_i rejects
the new reservation; existing contracts/accounts remain in force.

`latency` supplies B_i: an **administrator-established upper bound on cumulative
non-modelled interference over this response window**. A safe deployment may
bound it over the entire period T_i. It includes interrupt/firmware latency,
nonpreemptible kernel segments, COMMIT/ledger contention, peer overshoots, dispatch
and context-switch cost, timer/IPI service, cache/memory interference and delays
from the housekeeping/controller environment. Same-Hart competing budgeted
execution is already in the sum. Ordinary Tasks on other Harts can still contend
for shared Kernel resources: merely setting a small B_i does not bound them.
M2 reception windows preserve interrupt reception during lock contention, but
provide neither fairness nor a time bound on full scheduling. Those assumptions
must be independently justified on a deployment; **this PR does not prove them**.

As an illustrative admitted set in milliseconds, Q=2, T=100, B=10 for priorities
9/5/3/1 with ceilings 9/5/9/9 gives responses 20/24/24/24. Changing a competitor
to Q=99,T=100 makes the set fail. These are arithmetic results under the stated
B assumption, not measurements. Profile.response is a runnable CPU-service
bound under those assumptions. It excludes waiting for next own replenishment,
application self-suspension, external arrivals and server completion. Analyze
those phases separately (including each server's own budget) for an end-to-end
request. Neither a successful Admit nor low observed QEMU latency supplies a
missing physical bound.

## Verification

Persistent host tests import production decision/accounting code and exercise
credit conservation, delayed replenishment/overrun arithmetic, conservative
admission, ceiling/cycle/depth checks, transitive donation and cancellation,
priority/affinity decisions, queue ownership, ABI encoding and retired slots.

`server/tests/guest/cell/realtime` creates actual User Teams and RX code mappings,
collects grants before freezing/admission, and runs the real public ABI. QEMU
exec checkpoints observe relocated U-mode PCs. They verify that a remote wake
runs the high-priority Task between two checkpoints of a lower-priority loop
containing no syscall, with credit still available, and that admitted checkpoints
remain on one non-boot Hart. Guest assertions also cover budget replenishment,
Yield/freeze, overload rejection, nested service dependencies, cancellation,
revocation/rebinding, server exit, cycle/ceiling rejection, restricted calls,
page faults and prepared versus undeclared floating-point execution.

Run the full host suite and builds from the repository root:

```
cargo test --locked --workspace --target x86_64-unknown-linux-gnu
cargo build --locked --workspace --target riscv64gc-unknown-none-elf
cargo build --locked --workspace --release --target riscv64gc-unknown-none-elf
```

With QEMU_RISCV64, QEMU_OPENSBI and the existing external fixture manifests set:

```
cargo test --locked -p kernel-memory-tests --target x86_64-unknown-linux-gnu \
  --test hardware realtime_reservations -- --ignored --nocapture
```

The existing CI ignored-test command includes this group: debug 1/4-Hart and
release 4-Hart. The single-Hart case must reject admission; multi-Hart cases must
complete the RT assertions. Commands, image hashes, tool versions, serial logs,
watchdog records and checkpoint summaries are retained in the normal guest
artifact. These provide functional evidence, not a real-board WCET certificate.
