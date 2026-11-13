# Flat/Nest memory contract and migration

Status: **M1 contract deliverables complete in [PR #145](https://github.com/shuyra-arche/kernel/pull/145), pending merge**, for [#142](https://github.com/shuyra-arche/kernel/issues/142).
See the [M1 acceptance matrix and activation plan](memory-m1.md) for each section 9
requirement and its evidence. Value codecs, bounded journal transitions and the
trusted backend proof interface are implemented and tested. The next syscall ABI
is not activated; Task/resource integration and physical backends remain M2–M6.
Current executable contracts remain [ABI v10 / Ledger v10](README.md#source-migration-and-abi-compatibility).

The reviewed source baseline is Kernel
`f61164b594226978d15040fbd9e5847e7974b738`
(the merge of [#144](https://github.com/shuyra-arche/kernel/pull/144)),
after the issue's original `ac381c7` / ABI v9 baseline. #144 already implements
Nest Whole/Mapping authorization from #141. Preserve that work; do not recreate
Memo authorization roots. Engineering guidance was read at
`.github@11d25fcfeb1ceeda05a9e137bace040cc7f5517d`.

## Implemented value contracts

These `no_std` values prepare direct references for the next ABI. They neither
implement `RegisterCall` nor enable operations in `EnvCall`. Passing their bytes
to an existing v10 descriptor argument is invalid: v10 uses its own layouts.
Successful decoding proves only representation. Every operation must still
resolve a live Pend from the caller's table and check current identity, scope,
rights, lifecycle and complete physical geometry at commit.

All fields below are fixed little-endian u64 words, independent of Rust struct
layout. Decoders require exactly the stated length; extra bytes reject too.
Identifiers use the existing positive, at-most-`isize::MAX` representation.
Coordinates must represent at least one complete ABI base page without byte or
coordinate overflow. On a narrower host, unrepresentable coordinates/identities
reject; u64 revisions and record identities preserve their complete width.

| Type / module | Bytes | Words | Meaning |
| --- | ---: | --- | --- |
| `flat::SlotPosition` | 24 | `[flat_pend, flat_id, page]` | Fixed Flat-origin position; no Task-local SlotId |
| `flat::SlotVersion` | 8 | `[version]` | Nonzero residency/geometry revision |
| `nest::MemoPosition` | 32 | `[nest_pend, nest_id, mapping_id, page]` | Fixed Mapping-origin position; requires Mapping scope |
| `nest::MemoGeneration` | 8 | `[generation]` | Nonzero logical association/publication generation |
| `flat::BlockRef` | 24 | `[flat_id, record_id, generation]` | Maintenance record locator; contains no live grant |
| `flat::BlockGeneration` | embedded | BlockRef word 2 | Nonzero maintenance generation, distinct from Slot/Memo revisions |

Position identity and expected revision are separate. Inspect accepts a position
without knowing its current revision. A mutation must additionally carry the
expected revision. FlatId, NestId, MappingId and PageId remain descriptive;
numeric equality does not confer authorization.

`SlotPosition::new` / `MemoPosition::new` validate the same representation as
their decoders. Fields are private so invalid values cannot be assembled by
ordinary safe field assignment. Their getters do not resolve any Kernel state.
`BlockRef::new` requires a nonzero record ID. Revisions start at one, preserve
all 64 bits, and `next()` returns `None` at `u64::MAX`. Server code must reject a
new transition at exhaustion before committing any effects. Live Slot stamps are
issued from the Flat-wide high-water mark, not the local arithmetic successor;
see [Split/Merge issuance rules](memory-wire.md#splitmerge-geometry-and-non-reusing-slot-stamps).
Forced cleanup may retain the terminal generation, but must never resurrect ordinary mutation.

`BlockRef` does not contain the branch, Task or effective range. Those facts
belong to the Flat's original maintenance record. Each use will supply a live
Flat Pend separately and verify the stored binding. A guessed reference, a
copied reference, or a reference whose Task died grants nothing. Record IDs are
not reused within one Flat; takeover advances the distinct maintenance generation.

### Fixed coordinates and complete geometry

Both position types provide `fits(offset, count, level)`. This is a pure checked
geometry predicate, not a saved permission proof. `offset` and `page` use the
same **original** Flat/Mapping origin. For a level-1 physical Page, `page=512`
covers `[512, 1024)` in 4 KiB ABI units. It fits a window `[512, 1024)` but not
`[513, 1025)` or `[512, 1023)`. Coordinate zero does not become 512 after Narrow.
The window may be broader or unaligned; the selected complete Page must fit.

The position must name a physical head aligned to `level.pages()`. In addition
to this predicate, Server must check actual object geometry, physical identity,
destination VA/IOVA alignment and backend limits. A 4 KiB coordinate is not a
promise to split a larger physical Page or a hardware translation leaf.

## Next call boundary

The following reserves the next coordinated ABI bump, not callable declarations.
The implemented subset and exact wire encoding are in [memory-wire.md](memory-wire.md).
Reserve ABI 11 only if still free when the provider
is activated. If another change has consumed it, choose a fresh version and
update all consumers together. Do not make production dispatch accept a
partially implemented next ABI.

Use Flat domain 12 and Nest domain 13. Retire domain 2 entirely. Use new operation
slots starting at 16 for the new memory contracts so old memory tags cannot be
accidentally reused during migration. Non-memory domains retain their documented
slots under the new global version prefix.

| Flat slot | Operation | Required grant / semantic input |
| ---: | --- | --- |
| 16 | Create | None FLAT; logical range and physical geometry; returns root Flat Pend |
| 17 | Delete | Flat DELETE; entire original object; asynchronous retirement retained |
| 18 | Inspect | Explicit Object / Slot / Block / Request query; matching observation authority |
| 19 | Occupy | Flat OCCUPY; SlotPosition + expected SlotVersion; Fresh only |
| 20 | Vacate | Flat VACATE; SlotPosition + expected SlotVersion + Held maintenance binding |
| 21 | Block | Flat BLOCK; fixed range + request identity; optional explicit orphan recovery reference |
| 22 | Allow | Same live branch/actor and BlockRef; enters Releasing |
| 23 | Push | Two independently authorized SlotPositions and expected versions; local → remote |
| 24 | Pull | Same two endpoints; remote → local |
| 25 | Split | Flat RESHAPE; full parent, expected FlatVersion, parent level and Held maintenance |
| 26 | Merge | Flat RESHAPE; 512 immediate siblings, expected FlatVersion, parent level and Held maintenance |

| Nest slot | Operation | Required grant / semantic input |
| ---: | --- | --- |
| 16 | Create | None NEST; Internal only |
| 17 | Delete | Whole DELETE; close the space, retain incomplete hardware work |
| 18 | Inspect | Explicit Object / Memo / Mapping / Request query |
| 19 | Reserve | Whole RESERVE; vacant address reservation; returns Mapping Pend and address |
| 20 | Release | Mapping RELEASE; entire original reservation; no premature address reuse |
| 21 | Insert | Mapping INSERT; MemoPosition + generation + authorized logical source; replacement also REMOVE |
| 22 | Remove | Mapping REMOVE; MemoPosition + generation; old publication settled |
| 23 | Publish | Mapping PUBLISH; MemoPosition + generation; optional live Flat Pend + BlockRef |
| 24 | Retract | Mapping RETRACT; exact Memo publication generation |
| 25 | Protect | Mapping PROTECT; generation and requested access within all ceilings |
| 26 | FaultRoute | Full Mapping FAULT; expected MappingVersion/route epoch; register actual caller or disable |

Retain current Nest Whole/Mapping rights separation and Pend target kind 4.
Do not reintroduce target kind 7, a Slot root or a maintenance Pend target.
Flat's current rights bits 0–5 retain their meanings. Proposed new rights are
OCCUPY bit 6, VACATE bit 7 and RESHAPE bit 8. BLOCK alone never grants read,
write, TRANSFER, VACATE or RESHAPE. These bits are **not enabled** in current
`FlatRights` and current Limits decoders continue rejecting them. Mapping-only
FAULT bit 12 is likewise proposed for route configuration, not enabled in v10
`NestRights`; it conveys no data or publication rights.

Source descriptors need explicit Flat, Share and Memo variants. Every variant
must retain original authority, fixed range, data ceiling and revocation lineage.
Memo-to-Memo association must reject cycles or resolve to an immutable original
source binding at commit; it cannot manufacture ownership or a root grant.
No descriptor accepts a bare TeamId as a substitute for either endpoint's Pend.

### Results and failures

The [request/result codec](memory-wire.md) assigns exact numeric codes and layouts
for the implemented operations, distinguishing these outcomes:

| Outcome | Meaning | Recovery |
| --- | --- | --- |
| Rejected | No logical mutation was accepted | Inspect if needed; retry with a new request after fixing the cause |
| Pending | Accepted work or hardware retirement remains | Query the same request/object; do not repeat it as a new operation |
| Committed | Logical transition committed exactly once | Use returned versions/identities; a reply-copy failure cannot roll it back |
| Failed | Accepted operation failed; hardware/resource obligations may remain | Inspect the original object; retain unresolved resources |
| ResultExpired | Request is too old to prove its result | Never execute it again; reconcile current state under authorization |

Required distinguishable causes are Denied, Dead, Busy, OutOfMemory,
NotAligned, NoRegion, Stale, SourceEmpty, DestinationOccupied, NotTransferable,
Unsupported, VersionExhausted, RequestMismatch, ResultExpired and ReplyFault.
An accepted Block/Allow cannot be represented as an ordinary no-effect Busy.
The implemented result codec uses a Pending outcome with the request identity,
separate from register delivery status. Client and Server must integrate that
distinction before activation.

Inspect must expose the current phase and revisions, including Fresh/Resident/
Missing and in-flight Slot states, logical Memo source separately from physical
publication, and Block phases Draining/Held/Releasing/Closed/Orphaned. Unknown
tags, unknown rights, nonzero reserved words and truncated descriptors reject.
Observation must not require the version being observed or claim content stability.

## Commit and failure boundaries

| Transition | Commit boundary and retained failure |
| --- | --- |
| Occupy | Allocate and zero a complete Page privately; under the source commit boundary recheck Fresh, version, grant and blocking before installing it and advancing the version. A loser releases only its unpublished allocation. Missing rejects. |
| Vacate | Under the physical/source commit boundary recheck Held binding, original VACATE authority, exact Page and all normal/maintenance/external obligations. Commit Missing and relinquish the unique allocation including maintenance-held retention. Any unproved obligation retains the Page. |
| Push/Pull | Prepare both endpoint records, then lock in a defined identity order and recheck both grants, object identities, geometry and versions. Move one unique holding and advance both versions atomically. Failure preserves both positions; PageOwner never changes. |
| Insert | Prepare all source and reverse-tracking records before commit. Under the target/source commit boundary check generation, INSERT and additionally REMOVE for replacement, live source qualification, geometry, blocks and fully settled old publication. Swap the logical source and advance generation once; preparation failure leaves the old source intact. |
| Publish | Register a uniquely identified physical responsibility before any hardware exposure, with source/target grants and all concurrent blocks checked at that same admission boundary. Hardware work occurs outside long-held locks. A possibly exposed failure retains Page and auxiliary hardware resources. |
| Retract | Stop this publication, invalidate and obtain its precise completion evidence. Do not install a Page-wide freeze or wait for unrelated writers. Preserve the logical source. If a backend cannot retire that scope, report Unsupported/required scope rather than silently withdrawing peers. |
| Protect | Widening repeats whole-group admission; narrowing retains old responsibility until invalidation completes. Old and new access must never be simultaneously unaccounted for. |
| Block | Reserve record/result capacity before closing ordinary admission. Closing admission is the acceptance point; later Busy/Pending leaves it closed. Track source-established historical physical publications as well as current associations. |
| Allow | Atomically enter Releasing, stop new maintenance admission, and retire this record's maintenance accesses. Only exact settlement removes its block. Other blocks and revoked grants remain in force. |

Permission checks and reservation cannot be independent observations. A racing
Publish is either outside the admission boundary or has a responsibility visible
to Block/Revoke. Pending, preparing, failed and retiring physical work all count.
Counters, record IDs and publication generations must fail closed at exhaustion.
No global commit lock may be held while waiting for remote fences, devices or I/O.

First implementation policy for holding moves: reject moves with any outstanding
publication or unproved failure. Supporting published moves later requires a
separate proof that every old publication remains attached to the same Page and
original grant and that historical source blocks follow it. This conservative
policy is not an implicit revoke. Cross-Team PageOwner adoption remains unsupported
until both consent, quotas and responsibility settlement have their own contract.

Split/Merge must preserve the Flat identity and fixed coordinate system. Use a
non-reusing geometry revision covering every affected coordinate, including newly
created child heads, so an old child reference cannot become valid after a
merge/split cycle. The implemented value contract uses one checked FlatVersion
successor as the common SlotVersion for changed heads; requests check the full
Flat revision and operate on exactly one level. Live issuance still belongs to
the Server commit boundary. Reject partial Pages, borrowed allocations, outstanding
publications and nonexclusive allocation receipts. Prepare metadata and validate
layout before changing the allocation. Memo logical spans survive; an incompatible
new geometry makes Publish fail explicitly until repaired. No implicit rewriting
of other Memo associations or PageOwner is permitted.

### Bounded retry contract

A request identity is `(calling Task identity, nonzero monotonic u64 sequence)`.
It is distinct from any Slot/Memo version. The caller knows it before dispatch,
so failure to copy a reply cannot erase the only way to identify an accepted call.
All next-memory mutations, including creation and deletion, participate.

The independently tested `abi::memory::journal::Journal` implements a fixed
window of 16 records and a monotonically increasing accepted
high-water mark, as specified by `abi::memory::RESULT_CAPACITY`. The live Task
attachment and resource commit integration are still unimplemented. Requests are
bounded at 256 bytes and replies at 128. Store canonical operation inputs,
result and Pending/Committed/Failed state. Output-buffer addresses are delivery
details, not operation identity.
Reserve capacity before side effects. Never evict a Pending result to admit work.

Within the window, identical retransmission only observes or advances the same
operation; changing its payload is RequestMismatch. Below the accepted high-water
mark, an absent record is ResultExpired, **never** a fresh operation. Skipped
sequence numbers below that mark are therefore unusable. A full window returns
no-effect Busy before accepting another mutation. Querying a result performs no
new authorization or resource mutation. A saved result is not a saved live grant.

The object owns maintenance/retirement work even if a Task/result record dies.
The journal does not duplicate resource state. Completion uses the exact accepted
operation identity; stale callbacks cannot clear a new generation. Cleanup on
caller death follows the original objects and publication records. Orphaned
Block takeover requires original range/branch management authority, drains old
maintenance accesses, advances generation and invalidates the old credential.

## Hardware and service activation gates

The [backend proof contract](memory-backend.md) specifies exact identities,
independent evidence kinds, safe combiners, refusal defaults and platform-specific
obligations. Its compiled Rust interface is not an enabled hardware provider.

Whole-group compatibility must use trusted platform facts for the exact physical
range, memory attributes and every requester. Two pairwise CPU-coherent relations
do not prove that two devices can share. Unknown relationships, MMIO side effects,
unsupported executable visibility and unsupported noncoherent handoffs reject.
Keep the old conservative admission rules until the replacement is proved.

| Evidence | Owner and scope |
| --- | --- |
| CPU retirement | MMU backend; removed translations plus completion on all relevant Harts and treatment of prior accesses |
| External retirement | Trusted route/backend; exact binding and publication token, translation invalidation and drained in-flight accesses |
| Data visibility | Backend for the actual participating memory domains, before access admission or handoff |
| Executable visibility | Instruction-cache/backend proof and existing W/X policy, separate from ordinary data visibility |
| Safe physical reuse | Page allocator/receipt plus all Retentions settled, no hidden maintenance reference and no unresolved failure |

SFENCE.VMA alone is not device quiescence or complete data-cache synchronization.
Queue completion, notification, user Ack, timeout, driver death and Arc counts
are not retirement proofs. Failure retains pages, page tables, contexts and
queues whose reuse remains unproved. Each external publication requires an
independent record; the current `external_writer: bool` cannot represent this.

Ordinary faults and Publish must report nonresidency without implicitly zeroing
Fresh or Missing. The fault protocol must be implemented together with that
change: registration authority, stable Mapping/Flat identity and coordinate,
Memo generation, Slot version, event sequence, notification coalescing, service
death and bounded waiting/failure behavior. The [fault route and wait encoding](memory-wire.md#mapping-fault-routes-and-bounded-recovery)
now specifies Mapping-owned registration, retained events, coalesced Sign hints,
finite deadlines and explicit failure reasons. Existing boot `Trap::PageFault`
delivery does not implement that recovery contract. Do not disable
materialization in isolation and leave callers waiting indefinitely.

Page Service must first prepare content in authorized private storage, then supply
the complete Page. Save success belongs to the storage service protocol. Swap
does not gain Kernel privileges. A maintenance reader uses the same Publish and
Retention path, cannot bypass a second block, and retires before Vacate/Allow.
Recovery code, stack, event delivery and work buffers must not fault on the very
pages they are responsible for recovering.

## Consumer migration and review slices

| v10 boundary | Next contract | Migration owner |
| --- | --- | --- |
| Cell Create / Flat Slice → Task-local SlotId | Flat Create → scoped Flat Pend; fixed SlotPosition | API/Client/Server, then all memory consumers |
| CellPosition packs object tag and generation | Separate SlotPosition/SlotVersion or MemoPosition/MemoGeneration | API/Client and descriptor-copy sites |
| Cell Push/Pull require two local Slot handles | Two live scoped Flat grants, including remote sources | Flat/Pend integration; no target Task scheduling dependency |
| Memo association retains concrete Page | Persistent logical source; Publish resolves current Page | Nest, source revocation and reverse tracking |
| Fresh faults materialize | Explicit Occupy or service supply; Missing never zero-fills | Fault handling and Page Service together |
| Cell Retract retains Freeze | Single-publication retirement; Block owns range maintenance | Nest/Page/Metal |
| Flat Block/Allow take a token only | Fixed range, request identity and generation-bound BlockRef | Flat, caller lifecycle and service clients |
| Share Open/Shut publication bookkeeping | Same Nest admission and physical responsibility machinery | Share/Nest; preserve original authority and ceilings |
| Current Capsule byte 11 = 10; Ledger = 10 | One coordinated fresh ABI/Ledger version | Kernel provider, platform image builder and every executable |

API, Client and Server belong in one coherent provider revision for activation.
Keep source-only value additions separately reviewable. Once the provider is
available, update consumers to an immutable revision, regenerate Capsules and
Ledgers and rebuild the assembled image. Kernel's tests have a pinned Vanllia
dependency; inspect that pin and its fixtures rather than assuming it tracks the
provider. Do not mix old executable descriptors with a new dispatch prefix.
Rollback requires the complete old provider/consumer/image set.

The [M1 acceptance record](memory-m1.md) closes the contract/encoding decisions.
Fault queues, scheduler/Sign integration, actual geometry bookkeeping, allocation
conversion and physical proof implementations remain M2–M6. New work implements
those owners against the fixed contract and records any deliberate contract change.

## Verification boundary

`api/tests/abi/memory_contract/` verifies exact byte layouts, identity/sentinel
rejection, full-width non-wrapping revisions, unchanged coordinate meaning after
window narrowing, complete large-Page geometry and continued rejection of ABI 11
by current dispatch. The request/reply suites also verify the implemented call
payloads, failure/status combinations, delivery status and canonical encodings.
They use the production public codecs and the actual bounded journal.
The backend evidence tests compile the production Rust proof interface with
explicitly synthetic attestations; they do not claim a physical implementation.

It does not prove live Pend validation, races, atomic commits, orphan recovery,
request replay, retirement, data visibility, QEMU or hardware behavior. Those
tests belong with the owners implementing the relevant transitions. In
particular, keep the #142 acceptance cases for two independent writable Memo
exits, Publish versus Block/Revoke, overlapping blocks, partial hardware failures,
lost replies and stale callbacks, service death, swap and shared IPC/virtqueues.
