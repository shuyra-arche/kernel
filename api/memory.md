# Memory contract

Flat and Nest provide the two independent aspects of a memory relation:
**Flat holds a source**, and **Nest associates and publishes it at an
address**. A Page is the concrete physical holding, Pend is the live
authorization, and a Retention records outstanding hardware access.
Changing one does not silently transfer the responsibilities of another.

## Objects and geometry

| Object | Authority and state responsibility |
| --- | --- |
| Flat / Slot | Logical source, current Page holding, Fresh/Resident/Missing and in-flight transitions |
| Page | Physical identity, allocation receipt, admission and retention of all accessors |
| Nest / Reservation | Translation space, exclusive address reservation identity and lifetime |
| Memo | Logical association, expected generation and concrete hardware publications |
| Pend | Rights, scope/data window, attenuation, distribution and live revocation ancestry |
| Retention / Metal | Exact CPU/External publication obligation and trusted completion evidence |

IDs and addresses identify objects; they are not capabilities or evidence
of safe reuse. A mapped Page may outlive its original owner while pending
hardware work or another authorized publication remains.

ABI positions use **4 KiB units**. A physical `CellLevel` has size
`2^(12 + 9*level)` bytes; Sv modes and physical availability constrain
the supported levels. A large Page spans multiple base coordinates.
CPU/IOMMU hardware leaf sizes are separate from physical holding identity.

## ABI 11

The current revision has one ABI. Native Flat/Nest operations use fixed-origin
SlotPosition / MemoPosition with independent versions. Flat owns Occupy/Vacate,
Push/Pull, Split/Merge and Block/Allow. Nest owns association, publication and
Reservation FaultRoute. Versioned RequestIds and a bounded journal report
Rejected/Pending/Committed/Failed outcomes and checked observations.

No Cargo feature selects the ABI. Calls, memory descriptors, Ledger and Capsule
ABI derive from `abi::VERSION`; Capsule format remains 2. Remove the former
`memory-v11` dependency feature and rebuild all components at the same source
revision. ABI 10 remains available only through old immutable source pins.

Native `Opcode` selects Flat domain 12 or Nest domain 13, slots 16–26.
[`Action` and `Request`](src/abi/memory/request.rs) define the current
256-byte request, and [`Reply` and `Observation`](src/abi/memory/reply.rs)
define 128-byte result buffers. The [Frame](src/abi/memory/call.rs) binds
the actual call number to the snapshotted request. Unknown or
noncanonical descriptors, reserved fields and mixed ABI versions reject.

The actual Task scopes each monotonic `RequestId`. The bounded result
journal preserves accepted work and independent delivery status; a
delivery failure does not imply that a mutation was rejected or rolled
back. Inspect the original request instead of blindly replaying a
side-effecting call. [Client](../client/src/memory.rs) performs one trap
and no automatic retry or v10 fallback.

Native [Share.Derive](src/abi/call/view/share.rs) can derive a scoped
Share from a live Flat or Share with a suitable creation license.
Derived sources retain their real permission and data ceilings;
a descriptive source identity cannot mint a new owner. Native memory
does not accept old Cell calls or Share Open/Shut adapters.

## Holding, association and hardware publication

- **Occupy / Vacate** materialize Fresh holding or release a settled
  holding to Missing. Missing is not implicitly zero-filled on fault.
- **Push / Pull** transfer an existing Page holding between authorized
  Flat positions in opposite directions. Transfer is not copying or a
  change to the address association or its past hardware publication.
- **Insert / Remove** create, replace by expected generation, or remove
  the Memo's logical source. The source can remain associated without
  a resident Page.
- **Publish / Retract** create and retire a concrete generation of
  CPU or External access. Retract does not remove the logical source.
  Protect checks the requested rights and necessary invalidation.
- **Block / Allow** manage a particular Flat maintenance phase, not a
  global Exclusive/Cooperative memory mode. Native `BlockRef` is bound
  to the actual Task, grant branch, range and epoch. An authorized
  recovery may resume or take over a retained record after proving the
  necessary prior responsibilities. Revocation is never undone by Allow.
- **Split / Merge** require whole physical geometry and exclusive,
  settled allocation receipts; a failed prepare cannot publish
  partial conversion or silently reassign Page ownership.

No single state transition substitutes for another: authority, Page
holding, Memo association, CPU/IOMMU translation and physical retirement
can have different lifetimes. A child Pend or exported source cannot
widen its original access, source-distribution or revocation limits.

## Coherence and retirement

Legitimate concurrent CPU/External readers and writers still require
the complete participating group, memory attributes and trusted
platform facts to support visibility and ordering. Pairwise
CPU-device compatibility is not a substitute for whole-group evidence.
The Kernel does not infer race freedom, cache coherence or writeback
from a permission bit or a PTE A/D observation.

The enforcing backend interface is
[metal::generic::memory](../server/src/metal/generic/memory/mod.rs).
Its proofs bind a concrete Page interval, publication generation,
endpoint/binding epoch and attempt. Distinct evidence is needed for
whole-group compatibility, data/instruction visibility, translation
retirement, in-flight access draining and auxiliary table/queue/context
quiescence. Completion must match the exact obligation and scope.

Pending/failed effects keep their Page, auxiliary storage and address
tickets until real settlement; a timeout, Sign, CQ word, Task death or
driver confirmation does not authorize physical reuse. Trusted work
runs outside lengthy global commit locks. Unsupported topology,
noncoherent sustained sharing, ATS/PRI or bypass must fail closed,
not assume a platform proof from user-supplied labels. QEMU validation
does not qualify physical hardware.

See [Nest retirement](../docs/nest-retirement.md),
[IOMMU ownership](../docs/iommu-failure.md) and
[Pend delegation](../docs/pend-delegation.md) for the current
failure and authorization boundaries. [Verification](../server/tests/README.md)
separates Host, RV64, QEMU and real hardware evidence.
