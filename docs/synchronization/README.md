# Synchronization acquisition audit

This is the **incomplete S0 evidence** for [#185](https://github.com/shuyra-arche/kernel/issues/185),
not an implemented lock model or a claim that the candidate DAG is safe.
The source snapshot is Kernel `852b396d20a1ae2954503a4dee9d970658c43558`;
the engineering standard is `.github@7967d812c6c9d40fde6a174483073b788af3e3cc`.
The inventory must be refreshed after the independent resource-boundary PRs land.

## Coverage

[Construction sites](constructions.md) enumerates all 96 textual `SpinLock` /
`RelLock` construction sites under `server/src` at that snapshot: 66 unranked,
13 `L3`, and 17 with another explicit Level. A site is not an instance count:
Task, Span, Page, per-Hart, per-size allocator, and lazy registry constructors
create different numbers of live instances. Repeated Page/Nest constructors
initialize the same semantic field in different creation paths.

This inventory establishes discovery coverage only. It does **not** establish
all transitive acquisitions, temporary-expression lifetimes, implicit Drop
acquisitions, or all IRQ/callback paths. Each family below names an acquisition
owner and a proposed audit direction; an unclosed row remains a blocker to S0.
None of these proposed names grants a `Before` edge. In particular, the
unranked storage locks in a ledger are not the ledger transaction lock.

## Verified paths and migration blockers

### Ledger before the one global COMMIT

[`ledger/table.rs`](../../server/src/basis/pend/ledger/table.rs) has separate
`serial`, `entries`, and `heirs` locks. `Table::lock/try_lock` acquire only
`serial`. `with_pair_uncommitted` orders the **actual Tasks** by TaskId,
tries the first serial lock, then tries the second unless the IDs match.
The first guard unwinds on second-try failure; it has not created a COMMIT
turn. `with_pair` enters global COMMIT only after that protocol succeeds.

[`pend/branch.rs`](../../server/src/basis/pend/branch.rs) invokes this protocol
for delegation. It reserves a ledger registration and heir before publication.
[`ledger/cleanup.rs`](../../server/src/basis/pend/ledger/cleanup.rs) uses a
single or immediate-parent pair for bounded postorder closure. Its ordinary
hardware withdrawal is outside those ledger/COMMIT scopes; Busy leaves the
closed authority and original obligations registered for retry.

The order candidate is `LedgerSerial -> Commit`, with an explicit
`LedgerSerialPair` protocol. `entries`, `heirs`, admission `usage`, entry
`charge`, and source tracking are separate storage classes to audit below
that transaction. Matching TaskIds are not by themselves a general proof
that arbitrary supplied lock instances are identical. The typed pair must
get its identity and lock reference from the same Task owner, verify the
same-object branch, and cannot accept a caller-selected `(TaskId, Lock)`.

### A reverse nonblocking acquisition exists

[`nest/memo/source.rs::Source::borrow`](../../server/src/basis/nest/memo/source.rs)
documents a precondition of the publisher's serial and COMMIT. If the source
actor is another Task, it then calls **that actor's** `pend.try_lock()` before
source/authority/Page admission. The actual native caller needs separate care:
[`entry/call/nest/next.rs::effect`](../../server/src/entry/call/nest/next.rs)
releases its preparation serial/COMMIT scope before calling
`Nest::publish_memo_report`. That owner takes COMMIT before calling
`Source::borrow`, without establishing a publisher serial in that method.
The documented serial precondition is therefore not established by this
native call chain. The directly confirmed acquisition is:

```text
COMMIT -> try(source actor LedgerSerial), when source actor != publisher
```

A caller that separately retained the publisher serial would add another
ancestor; do not infer it merely from Source::borrow's comment. The `None`
Level hides this reverse edge from the legacy order predicate. This is not
ordinary `LedgerSerial -> Commit` and is not a same-instance COMMIT reentry.
A failed try returns Busy without waiting, but the successful branch performs
further locking; it cannot be treated as a leaf exception without auditing
those successors. Do not add `Before<LedgerSerial> for Commit` to make it
compile. Prefer preparing both actual Task owners and taking their controlled
TaskId pair **before** COMMIT, then repeat source and publisher validation.
The observed source and publisher identities must be revalidated after taking
the pair; all callers and the retained hardware admission need inspection before
that change; this audit has not proved that rewrite equivalent.

### Opposite Pend state/parent nesting exists

In [`pend/authority/mod.rs`](../../server/src/basis/pend/authority/mod.rs),
`borrow_device` holds the lender's `state` while setting the child's `parent`.
`finish` releases its own `state`, then an `if let` scrutinee's
`self.parent.lock().take()` temporary stays alive through the body that locks
the lender's `state`. The semantic-field graph therefore contains both:

```text
PendState(lender) -> PendParent(child)       borrow_device
PendParent(child) -> PendState(lender)       finish
```

This is an obstacle to those **classes** forming a DAG, not proof of a current
cross-Hart deadlock: production device borrowing/settlement has COMMIT and
resource lifecycle preconditions. State a real race only with a counterexample
that accounts for those preconditions. A candidate repair is to delimit the
first state update with `{}` and take the parent record into a local before
locking its lender. Its atomicity and rollback still need owner-level review.
The host ancestry/selection/maintenance cases must retain their actual borrow
and settlement assertions. [PR #198](https://github.com/shuyra-arche/kernel/pull/198)
implements this local scope separation independently, with three storage-overlap
regressions that fail against the snapshot implementation. It is not part of
this documentation snapshot and does not establish the rest of the DAG.

### Leaving COMMIT does not release Ledger or restore IRQs

[`pend/branch.rs`](../../server/src/basis/pend/branch.rs) can release COMMIT,
withdraw register mappings and wait for their invalidation, and then reacquire
COMMIT and revalidate both Tasks, the grant, and the source. The two ledger
serial guards from `with_pair_uncommitted` remain alive through that closure.
[`entry/call/nest/next/lifecycle.rs::effect`](../../server/src/entry/call/nest/next/lifecycle.rs)
also holds the actor's serial while preparing a private Nest, whose trampoline
PTE installation completes a fence outside COMMIT.

These scopes are not evidence of an exclusion-free hardware boundary.
Dropping the inner COMMIT TrapGuard preserves the enclosing serial lock's
IRQ-off state. M5 requires auditing and, where necessary, retaining durable
preparation/rollback records before releasing serial as well. Do not just
release serial early: delegation's reserved registration/heir, operation
lease, authority generation, closure and late failure responsibilities must
survive and be revalidated before publication. A typed Root must not conceal
this existing lifetime.

### Source gate and storage locks have real successors

[`pend/source/mod.rs`](../../server/src/basis/pend/source/mod.rs) has
`revoked` (publication gate), `inflight`, `phase`, `bindings`, `blocks`, and
`origins`. For example `enter_publish` keeps the gate while checking branch
permission, block admission and then the inflight list. `permission_ungated`
can keep phase while consulting a Pend authority. `begin_withdraw` returns a
real gate guard whose lifetime extends into its caller. These cannot all be
marked as interchangeable leaf locks.

[`pend/source/admission.rs`](../../server/src/basis/pend/source/admission.rs)
has a separate `RangeBlock` with `pages/native/uses/stages`. `vacate` and
`reshape` pass the actual `pages` guard into Flat operations. `retain_staged`
keeps `stages` while inspecting `pages`; `recover_origins` in
[`provenance.rs`](../../server/src/basis/pend/source/provenance.rs) keeps
`origins` while accessing each provenance `grant`. These nested directions
must be represented or split. A source-authorization index is not a second
Page or Memo authority (the #104 conclusion still applies).

### Allocator locks are implicit acquisitions

[`allocator/block/inner.rs`](../../server/src/metal/allocator/block/inner.rs)
holds `pool` when `prime` allocates a Frame and updates Tally. Draining also
updates Tally and returns a Frame under `pool`. `suck` takes a pump's detached
list and releases its pump guard **before** calling `push`; do not infer a
`BlockPump -> BlockPool` nesting from call order alone. Frame and Spare invoke
allocation statistics under their own guards. `Tally::with` only manipulates
its metadata under its actual unit lock.

`Vec::try_reserve`, `Arc/Box` allocation, replacement/clearing of collections,
and the last strong `Arc` Drop can reach these mechanisms from many resource
locks. The bump allocator has a distinct early-bootstrap lifetime. The typed
resource chain alone cannot certify those implicit paths. The future graph
needs explicit allocator directions and an audit of allocation/prohibited
contexts; `None` is not an acceptable fallback class.

### Specific bootstrap, shutdown and implicit-allocator observations

[`entry/boot/devices/entry_mmio.rs::install`](../../server/src/entry/boot/devices/entry_mmio.rs)
extracts and seals its staging vector under STAGED, then releases that guard
before Flat/Nest installation. In contrast, external-device `init` keeps NESTS
while creating an external Nest and binding its trusted controller. `stage`
and `grant` can grow their vectors under their own storage locks. Bootstrap
ownership is not proof that these are allocation-free bottom classes.

[`mmu/identity/asid.rs`](../../server/src/metal/riscv/mmu/identity/asid.rs)
locks its Recycle for allocation/release. Recycle's **first** allocation grows
its bit vector while that guard is live; release only clears an existing bit.
No fence occurs in those methods. The ASID class therefore still needs the
allocator successor, even though steady-state ID allocation is just a scan.

[`nest/index/source.rs`](../../server/src/basis/nest/index/source.rs)
preallocates new records and snapshots before upgrading strong references.
Its rebuild can nevertheless dispose of dead Box records while the head is
locked, so deallocation must be included in the implicit allocator audit.
[`unit/task/chain.rs`](../../server/src/basis/unit/task/chain.rs) uses TaskState
locks one at a time to update predecessor/successor links, but `Chain::drop`
also dequeues through those locks. Reap/husk/ready-queue owners that embed a
Chain have those same hidden successors.

[`room/shutdown/mod.rs::observe`](../../server/src/basis/room/shutdown/mod.rs)
limits PROGRESS to its local counter/time decision. The stalled callback runs
after that guard's block ends. The later non-returning Hart barrier and finish
hooks do not retain that guard. This only establishes this method's local
boundary, not callback safety for every possible caller.

### Pairing and actual instances

[`sync/pair.rs`](../../server/src/metal/riscv/sync/pair.rs) orders two **real
Flat lock addresses**, rejects equal addresses in all builds, and encloses
the guards in one closure. The production `peer` predicate requires exactly
one held Flat predecessor with that precise smaller address. It rejects a
third peer and a higher-class ancestor. Replacing the closure with RAII must
retain distinct-state mutable access, target/parent lifetimes, reverse
release order, and the IRQ restoration order. A general same-class
`Before<Flat> for Flat` is not equivalent.

## Remaining family audits

The following are **work still required**, not blanket order permissions.
All constructors have a family below; transitive acquisition completeness
has not yet been established for any family merely by listing it.

| Family / actual owner | Next acquisition and lifetime question / proposed disposition |
| --- | --- |
| Global COMMIT | One reentrant instance, FIFO and shared-only guards; enumerate all implicit reentries and replace typed reentry by an actual ancestor witness. Keep the global scope until a transaction equivalence proof exists. |
| Flat state | Residency/shape/transfer can allocate and touch Page/maintenance state; preserve the exact two-instance address protocol and same-instance single-lock branches. |
| Flat source export | Split `authorization`, mapping records and authority discovery semantically; inspect last-Arc Drop and failed mapping restoration. Renaming Sharing to Export does not discharge these locks. |
| Nest layout / pending cleanup / state | Audit layout-to-state, state-to-Span/Flat and backend completion; external Nest's `pending_cleanup` currently differs in Level from ordinary construction. |
| Span faults / exports / state | Distinguish fault policy, export discovery and Reservation residency; audit Memo replacement/Drop under the Span guard. |
| Nest authority ranges / organizations / source head | Registration, binding cleanup, last-Nest upgrade and intrusive source retirement; `L3` source head needs a specific class. |
| Activity registry / delivery / history | Snapshot, collection, consumer death and delayed delivery; do not hold a storage guard over copy-to-user or MMU/IOMMU effects. |
| Fault queue progress | Progress ownership survives publication/consumer loss; inspect cancellation/continuation and Sign wake paths. |
| Pend state / parent | Opposite class directions above; immutable ancestry traversal takes one short state observation per node. |
| Ledger serial / entries / heirs / admission / charge | Distinguish exclusion from storage; pair by TaskId; account for `Registration::commit` callback and Charge/registration/heir Drop. |
| Source gate / phase / flights / bindings / blocks / origins | Publication, adoption and withdrawal use different successors; see verified paths above. |
| Hold table holds / sources | Slot registration, source discovery and retirement; M2 removes old Slot adapters but retains current Memo export discovery. |
| Provenance grant / uses | Live branch recovery and retained uses are distinct; account for source `origins -> grant` and hardware withdrawal outside snapshots. |
| RangeBlock pages / native / uses / stages | Include `pages -> Flat` and `stages -> pages`, active epoch and preparation rollback; do not label all four as bottom exceptions. |
| Page membership / Change blocks / live changes / retention totals | Group compatibility, gate closure, cancellation, telemetry and Drop; live-change `L3` is discovery, membership is actual retention state. |
| Scheduler slot / admission | Per-Hart dequeue, handoff and affinity/budget contracts; any COMMIT use must precede Scheduler, with no guard surviving an actual context switch. |
| Deadline queue | Distinct `L3` class; callback/consumer wake acquires other owners only after detaching the due entries. Verify all expiration/retry paths. |
| IRQ router controller / progress / sources / source state | Firmware/MMIO operations and retained claims are not interchangeable locks; enumerate front-half, deferred service, rearm and Drop paths. |
| Shutdown progress | Retained cleanup stage versus shutdown effects; inspect early return, Busy and final firmware reset separately. |
| Sign state / routing / delivery / pending queue / waiter maps | Distinct semantic classes; intrusive pending queue nests endpoint delivery. Check Scheduler and UnitState successors, cancellation and last-Arc Drop. |
| Task state / receipt storage / result / journal / heir / retirement / schedule / extensions / memory | Keep Task state distinct from request journal and extension storage; creation, replay, async completion and Task death need explicit lifetime evidence. |
| Task registry / administrative control | Weak Task upgrades and receipt cancellation can run Drops; registry rank alone does not prove those callbacks. |
| Team state / tasks / children / held / termination / operation / exit policy | Pending termination queue nests per-Team termination; TeamTasks checks member UnitState. Operation is a transaction scope, not a leaf storage lock. |
| Lifecycle pending queues / husks / diagnostic ledger | Intrusive queue membership accesses Task scheduling links; snapshot before retirement/notification. Distinguish deferred work and telemetry from resource authority. |
| Clone pending queue | `Registration::commit` can hold entries while publishing to Clone; receiver/Task death and grant rollback require separate evidence. |
| Boot MMIO stage / external regions / external Nests | Bootstrap-only ownership, but Nest preparation and vector growth occur while some staging guards are live; no normal Root factory may be exposed to these owners. |
| Allocator pool / pump / Frame / Tally / Spare / bump | Verified local directions above; complete all caller, deallocation and diagnostic paths before declaring bottom exceptions. |
| ASID allocator | Short recycle storage; verify creation/destruction and no fence under its guard. |
| IOMMU controllers | Guard can invoke controller/fabric methods; inspect completion, fault and callbacks before assigning a class or bottom exception. |

## Execution boundaries and diagnostic coverage

The current primitives disable IRQs with a real TrapGuard, reserve a FIFO turn,
and release the turn before their TrapGuard restores the saved state. RelLock
reentry reuses the same Hart's **same real instance**, maintains recursion count,
and exposes no mutable Deref. Guard `!Send` is enforced by its raw-pointer
marker. None of those facts provides a controlled typed Root today.

`metal/riscv/switcher/trap.rs::trap_handler` distinguishes a user Task trap
from an interrupted Kernel continuation before entering `entry/trap`.
`entry/trap::service` can process deferred external work at a reception
boundary; allocator and last-reference Drops can run within existing scopes.
Boot has a separate initialization boundary. These are different execution
conditions: a constructor callable from any resource owner, or a new Root for
every Kernel IRQ, would bypass held ancestors. A Root prototype still needs
unforgeable boundary construction, `!Send`, parent/target lifetime checks,
and negative compilation tests; none is delivered by this inventory.

At the snapshot, Debug `depend` uses a per-Hart, preallocated HeldSet; it
checks duplicate instances and the maximum ranked Level, while `None` skips
the numeric comparison. The initial capacity is eight; `reserve_depend` can
grow it during Task preparation. Push refuses to allocate. Early locks before
`init_depend` have no held pool. Callback/IRQ safety of the mutable Hart-local
tracking must remain in the execution-boundary audit.

[PR #196](https://github.com/shuyra-arche/kernel/pull/196) independently fixes
try-lock checks before FIFO reservation and makes host order cases import the
production legacy predicate. It does **not** supply a DAG, classify `None/L3`,
or prove the complete acquisition graph. The host Hart/IRQ shell is still a
fixture. Release compile-out and real 1/4-Hart QEMU remain separate obligations
of the forthcoming typed-model change.

## Closure criteria for S0 and the next implementation

S0 stays open until each construction row is connected to all acquiring
methods, retained guards, transitive successor locks, failure/try paths and
implicit Drop/IRQ boundaries. The current findings already disprove a purely
mechanical conversion of legacy Level integers to the proposed graph.

1. Resolve the cross-Task source-borrow reverse try protocol and the Pend
   state/parent class cycle without changing authorization or failure retention.
2. Resolve ledger-held hardware waits with durable preparation/rollback and
   repeat validation; test concurrent Revoke, Task death and late completion.
3. Complete the family audit, including the 66 unranked and 13 `L3` sites;
   record allowed edges, incomparable classes and narrowly justified exceptions.
4. Establish one reviewed acyclic relation with an explicit reachability closure.
   Static `Before` and Debug comparisons of **every** held instance must use it.
5. Compile a real Root/Lock/Guard/Pair prototype and its negative cases before
   migrating one actual Ledger -> COMMIT -> Nest/Flat route.

No COMMIT shrink is proposed here. No execution-time bound or performance gain
is inferred from CI/QEMU samples. This documentation changes no source API,
wire ABI, permissions, lock atomics, IRQ behavior or dependencies.
