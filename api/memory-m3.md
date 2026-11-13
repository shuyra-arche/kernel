# M3 residency and publication delivery

M3's staged provider implementation is delivered on top of the
[Reservation migration](memory-reservation.md) and live lineage follow-up
`91c6e82f16108d51c3ec146fc4bdd563c25fd334`. It supplies explicit Flat residency,
persistent Memo association, versioned replacement, physical publication and
reverse cleanup, with the ordinary Block/Allow prerequisite for Vacate.
Running ABI, Capsule and Ledger remain v10. This is source and host verification
of the inactive v11 path, not approval to activate it or a claim of native guest
or physical hardware acceptance.

## Residency and maintenance prerequisite

- Explicit Flat creation allocates no payload Page. Its private compatibility
  marker prevents v10 implicit materialization, transfer and reshaping from
  mutating it. The marker separates provider generations, not sharing modes.
- The original Flat state owns residency and sparse Slot stamps. Fresh heads
  start at one; successful Occupy/Vacate stamp the changed head with the next
  Flat revision. Preparation failures and stale requests preserve versions.
  Missing never becomes Fresh or receives implicit zero-filled data.
- Occupy prepares and zeroes an owned Page before COMMIT, then rechecks live
  OCCUPY, complete geometry, source admission and the expected SlotVersion.
  Losing preparations are released without changing the holding.
- Push/Pull check complete endpoints and both versions under COMMIT and ordered
  Flat locks. In-flight publications reject without changing either holding.
  Moving into Missing restores the exact Page and allocation owner without zeros.
- Block closes the selected source window under COMMIT and records a stable
  BlockRef bound to the original Task and actual Pend branch. Ordinary accesses
  retire outside the Task ledger and journal locks. Held requires exact hardware
  settlement; a failed attempt retains the Draining record and closed admission.
- Vacate requires live VACATE and BLOCK authority, the original Held BlockRef,
  matching Flat identity, complete geometry and no other overlapping block.
  The storage commit consumes this maintenance record's exact Page retention.
  It removes the registry's weak Change upgrade path and uses `Arc::get_mut` to
  exclude all other strong/weak Page and Change holders while admission stays
  closed. Refusal restores retention without allocation or version changes.
  Hardware accounting must already be idle; reference counts are not proof.
- Allow closes only the matching original record after its obligations settle.
  Revocation leaves native blocks closed. Orphan takeover, maintenance publication
  and holding transfers carrying maintenance credentials remain M4 work.

## Logical association, publication and lifecycle

Insert retains a fixed Source and reverse binding, without retaining a current
Page. Fresh/Missing residency can be associated but Publish returns SourceEmpty
until a resident Page exists. Source coordinates are descriptive; the original
actor, live authorization lineage and sticky access ceiling govern every use.
Memo source export additionally requires Reservation PUBLISH and preserves all
upstream restrictions.

Replacing an association requires INSERT and REMOVE, an exact generation and a
quiet, settled Memo. Binding/index preparation precedes the atomic swap. Stale,
busy, denied and exhausted requests preserve the previous association. Remove
advances generation without moving a holding or initiating unrelated retirement.

Publish resolves the current resident Page and registers its exact publication,
publisher and source obligations before hardware effects. Retract retires that
publication independently of unrelated writers and keeps the logical binding.
Protect checks both source and target ceilings; a quiet logical association can
change access without allocating or publishing a Page. Published protection
uses the existing exact CPU invalidation path. External submission alone never
produces a committed usable publication: the backend must confirm readiness.

Physical Page identity and accepted access remain in Memo through retirement,
including whole-namespace withdrawal that moves the receipt into Nest's fence
batch. Source/Task/Reservation reverse indexes discover the original binding;
cleanup rechecks it before clearing, so an old binding cannot clear a replacement.
A failed fence preserves the Page and association until exact completion.

The staged entry also wires Internal CreateNest, Reserve, complete Reservation
Release and Internal DeleteNest. Reserve publishes the new namespace and its
actual derived Pend under the same commit boundary. Release closes admission
before retirement and holds the address until own Memos and foreign exports
settle. Reservation identity is never recycled into a new reservation at the
same address. Nest and Reservation revisions cover their respective topology,
lifecycle and Memo changes; completions may finish an accepted generation at
exhaustion but cannot wrap or reopen admission.

## Observations and recoverable results

Flat, Slot, Block, Nest, Reservation and Memo observation use the owning records.
A busy Slot reports its retained Page, hardware obligation count and closed
admission; a busy Memo reports its exact physical Page, publication access and
phase. Reservation phase/version are sampled under the Nest state lock used by
completion. Reservation access describes the creation ceiling restricted by the
inspected Pend's write ceiling; current per-position access belongs to Memo.
Observations confer no authority and do not settle obligations.

The actual staged dispatcher copies the complete descriptor before any output
or effect, checks its trap opcode and rejects realtime callers. Each Task owns
one lazy, bounded 16-record Journal. Admission reserves a result before mutation;
replay never executes the operation again, including after a reply-buffer fault.
Result inspection remains available to the actual Task after resource authority
is lost.

Atomic changes record their terminal result under COMMIT. Effectful operations
record Pending with the accepted identity/generation before hardware effects,
release ledger/journal/COMMIT locks for CPU fences, and record success before
reply delivery. Rejection before acceptance is distinct from failure after
acceptance. A bounded attempt that cannot finish returns Failed with its retained
accepted value, often Busy; that does not undo a block, publication or retirement.
Later owner maintenance can finish the resource without rewriting that attempt's
terminal result. Query the current resource and submit a new RequestId with its
current version to continue; replay of the old RequestId only returns its result.
The retained value identifies acceptance, not a fresh object-state snapshot.

## Verification and limits

| Evidence | Coverage |
| --- | --- |
| Production Flat/Page host tests | Fresh/Missing, racing Occupy, stale versions, transfer of exact holdings, rejection with old publications, allocator returns and strong/weak ownership |
| Production maintenance retention tests | Allocation-free consumption of the sole Held Page block; refusal for overlapping blocks, in-flight Change, Page observers, stale versions and unfinished hardware receipts |
| Production Memo/Page host tests | Persistent binding, failed replacement, independent writers, exact physical retirement, failed-fence observation and whole-namespace withdrawal retention |
| Production Pend/ledger and lineage tests | Original actor/branch, narrowing, revocation, read-only/non-distributable export restrictions and intermediate Task/Reservation loss |
| Production staged dispatcher/Journal tests | Lost output, overlapping input/output, replay, Pending/Failed retention, result inspection, pre-effect rejection and lock release at an effect boundary |
| RV64 debug and release builds | Actual provider integration, including inactive native entry paths, with production warnings denied |

Host Memo source/route fixtures are inert; ledger tests use namespace lifetime
fixtures; dispatcher tests use Task, user-buffer and resource-effect fixtures.
Their combined evidence does not claim a native trap-to-ledger-to-MMU execution.
Scripted receipt completion is not a hardware fence. No QEMU or physical run is
claimed; the 14 hardware tests remain ignored by the host suite and their guest
currently consumes ABI v10.

The [M4 follow-up](memory-m4.md) implements maintenance publication,
credential-bearing transfers, takeover and per-obligation retirement; its
verification boundaries and durable donor provenance are recorded there. Remaining staged Flat
Delete/Split/Merge and FaultRoute paths still reject; trusted External lifecycle
and fault/service integration belong to the corresponding M4–M6/activation gates.
The [M1 migration gates](memory-m1.md#migration-order-and-acceptance-gates) still
require coordinated provider, consumer and image switching. Changing the ABI
constant alone is insufficient. Issue checkboxes and merge decisions are not
changed by this delivery.
