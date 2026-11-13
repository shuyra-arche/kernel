# Next-memory backend proof contract

M1 contract, not an enabled hardware path. The compiled trusted Rust interface
is [metal::generic::memory](../server/src/metal/generic/memory/mod.rs), exposed to
platform code as `kernel::entry::boot::memory`. Exporting it installs no provider.
The default `Unsupported` backend refuses every operation. There are no userspace
proof bytes, coherent flags, callbacks or new Pend targets.

## Identity and trust boundary

A Request binds the exact Page identity and physical interval, publication ID and
generation, endpoint epoch, Page membership revision, non-reusing attempt and
required scope. Coordinates and extents are checked, but numeric values never
prove platform facts. Trusted integration resolves ordinary RAM, attributes,
requester topology, CPU leases and alias coverage from its boot/platform records;
it must not trust a Unit to label memory Ram or nominate a coherent requester.

Endpoint::Cpu binds a Nest identity and its CPU lease epoch, covering every Hart
that may use that space. Endpoint::External binds controller, requester and the
immutable boot-issued binding epoch. Rebinding, ASID reuse, publication reuse and
new retirement attempts must not make an old key match. IDs/counters never wrap;
new operations fail before effects on exhaustion. Existing obligations remain.

Page owns one complete membership list. Group includes the candidate and every
prepared, publishing, active, retiring or unresolved-failure member, regardless
of source Flat, Nest or alias. `Group::new` rejects duplicate publication IDs,
non-RAM, W/X conflicts and more than 64 total members. This finite first-version
bound returns Capacity before admission; no peer is evicted to admit another.
The backend assesses the entire group, not a pairwise compatibility matrix.

The API returns opaque, non-Clone/non-Copy Evidence values. Only the unsafe trusted
provider may attest them. The unsafe boundary is explicit: echoing a key is not a
proof. Each attestation is a claim about the physical system that M5 must establish.
Safe constructors and host fixtures cannot establish those facts.

## Independent obligations

Every Backend method performs a bounded progress step and returns
`Result<Option<Evidence<K>>, Error>`. None means pending; errors preserve resources.
No default method returns success. The caller cannot hold a global commit lock
across backend work, remote fences, device progress or storage I/O.

| Method / evidence kind | Required physical fact |
| --- | --- |
| compatible / Compatibility | The complete group can coexist continuously on the exact RAM and attributes under trusted topology and platform ordering constraints |
| data_visible / DataVisible | Prior writes required by this admission/handoff are visible to every participating memory domain before exposure |
| instructions_visible / InstructionsVisible | Code bytes are instruction-visible on every relevant executing Hart, separately from data visibility and translation invalidation |
| translations_retired / TranslationsRetired | The selected publication is unreachable through translations/caches covered by its exact endpoint and scope |
| accesses_drained / AccessesDrained | Prior accesses, including ones not yet observed by the translator, cannot touch the retired range after completion |
| auxiliary_quiescent / AuxiliaryQuiescent | The exact detached table/queue/context allocation is no longer reachable by walkers, commands, delayed writes or future old-binding submissions |

`visible` combines matching compatibility and data evidence, plus instruction
evidence for executable access. It rejects mismatched fields and non-RAM. The
result is still not authority: the owner must recheck current grants, blocks,
Page identity, source/target versions and the same membership revision before
exposure. If any changed while proofs were prepared, discard the bundle and
reevaluate; do not extend a stale group proof to new participants.

`retired` requires both exact translation and access-drain evidence. It settles
one publication only. It does not freeze other writers, discharge peers or free
a Page. The owner consumes the result once against the still-matching obligation;
a duplicate/late callback cannot decrement a later generation's count.

`auxiliary` additionally rejects Publication scope: detached CPU storage requires
CpuAddressSpace/CpuGlobal coverage, and external context/queue storage requires
ExternalBinding coverage. The region identifies the auxiliary allocation itself.
It neither substitutes for payload retirement nor frees allocator-owned memory.
Drop of Evidence, Visible, Retired or Quiescent performs no cleanup and proves
nothing about unresolved physical access.

## Backend-specific evidence and conservative policies

| Backend / existing boundary | M1 requirement for the new interface | Activation decision |
| --- | --- | --- |
| CPU MMU, `generic::mmu::PageTable` and `fence::Shootdown` | Prepare edits and retired-table storage before commit; publish ordered edits; cover every old CPU lease and the exact local/remote invalidation; separately establish completion/order of prior memory accesses and instruction visibility | No automatic adapter from Shootdown success to all proof kinds |
| RISC-V CPU | SFENCE.VMA addresses translation visibility only; remote Hart acknowledgements must name the exact request/lease epoch. Data and instruction synchronization need their own architectural/platform basis, including the relevant remote instruction fences | Unknown or incomplete visibility path is Unsupported |
| RISC-V IOMMU, `generic::iommu::Hardware` and `Quiescence` | Exact binding/publication/batch, removed translation, invalidation plus IOFENCE.C PR/PW completion, and independent platform quiescence covering pre-IOMMU/in-flight accesses | A CQ completion word or IRQ cannot mint AccessesDrained by itself |
| External binding teardown | Old context inaccessible, queues and tables quiescent, old submission authority removed, isolation group covered before reuse/rebinding | Preserve current Quiescence obligations; never infer reassignment safety from a new software epoch |
| RAM shared by CPU and External requesters | Whole-group ongoing visibility, actual attributes, all aliases and platform ordering/atomic limits are trusted facts | First activation allows only verified platforms; two CPU/device proofs do not imply device/device compatibility |
| Noncoherent sustained sharing, ATS/PRI, bypass, unknown topology | No supported proof path in the first contract | Unsupported; serial handoff also remains Unsupported until a complete backend path exists |
| MMIO or unknown memory kind | Side effects and device ordering cannot be treated as ordinary shared RAM | Rejected by the new admission helpers; retain separate existing authorized MMIO paths |
| Executable access | Explicit instruction visibility and W/X; one member's write conflicts with another's execute | Reject W/X group; no fallback to data-only proof |

Backend errors map to memory failures without changing acceptance status:
Invalid → Invalid, Capacity → Busy, Unsupported/RequiredScope → Unsupported,
Stale → Stale, Fault → Hardware, TimedOut → TimedOut. Before effects they may be
Rejected; after possible exposure they are Failed with the original responsibility
retained. None remains Pending for publication/retirement operations. A timeout
cannot synthesize Retired or Quiescent evidence.

Error::RequiredScope reports Publication, CpuAddressSpace, CpuGlobal or
ExternalBinding when finer retirement cannot be proved. The owner must reacquire
appropriate authority and coordinate the larger boundary, or return Unsupported.
A coarser fence is not permission to remove unrelated mappings. Fault/TimedOut/
Unsupported/None never become success through retry limits, driver death, Task
exit, an Ack, a timer or a reference count.

The new interfaces do not replace existing v10 backends. M5 must implement and
validate the required proofs for each enabled path. Until then, new hardware
sharing remains disabled rather than silently adapting an incomplete old proof.

## Admission, retirement and safe reuse

1. Under the existing owners' commit boundary, validate live authority and
   geometry, reserve result/member/cleanup capacity, issue unique IDs and register
   the candidate responsibility before hardware can observe it. Save the full
   Page group revision, including competing maintenance/block admission.
2. Perform hardware preparation and proof polling outside long-held locks. Keep
   the accepted operation, Page and all auxiliary allocations alive throughout.
3. Reenter the commit boundary and check the exact key and current source/target,
   grant and block state. Commit usable publication only with valid visibility.
   A race either failed admission or left a responsibility visible to cleanup.
4. Before first possible hardware exposure, mark the responsibility as potentially
   published. Any failure from then on follows retirement/quarantine, not the
   cancellation path for an unpublished allocation.
5. Stop new access, request exact retirement and consume both physical proofs.
   Retain each other publication independently. Release auxiliary storage only
   after its own exact quiescence proof; keep failed queues, contexts and tables.
6. Vacate/reuse additionally requires no ordinary, maintenance, historical or
   failed responsibility and a uniquely owned, releasable allocation receipt.
   End the Slot holding and the Block's retained reference in the same commit.
   A safe hardware proof alone is not an allocator release certificate.

For Split/Merge, the allocator must expose a prepare/commit contract: prepare
validates full aligned layout/provenance, exclusive receipts and destination
metadata while leaving old receipts unchanged; commit consumes those receipts
and installs the complete replacement group without allocation or another
fallible step. Failure before commit preserves the old group. No supported
atomic conversion means Unsupported. The new Page identities, common Slot stamp,
Flat revision and existing maintenance tracking change at that single commit.
Neither geometry conversion nor transfer silently changes PageOwner.

## Evidence available at M1

[Host evidence tests](../server/tests/memory/evidence.rs) compile the production
interface and check refusal defaults, complete-group bounds, W/X/MMIO rejection,
exact key matching, separate visibility/retirement obligations and a three-member
case that two pairwise checks cannot approve. Synthetic attestations are confined
to those pure tests; they are not a hardware implementation or physical proof.

RV64 compilation checks the same exported interface with production warnings
preserved. M4 race/retention tests and M5 CPU/IOMMU/platform execution remain
mandatory before activation. No QEMU or physical evidence is claimed by M1.
