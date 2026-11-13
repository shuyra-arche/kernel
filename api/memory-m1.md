# M1 acceptance and activation plan

**M1 deliverables are complete in PR #145, pending merge.** This is completion of
contracts, migration decisions and their executable checks, not completion of
runtime integration. ABI/Ledger remain v10. At M1 delivery, M2–M6 remained open;
the later [M2 acceptance record](memory-m2.md) tracks organization and authority
delivery and its remaining activation gates. This document maps
[#142](https://github.com/shuyra-arche/kernel/issues/142) sections 9 and 13 to the
implementation evidence and makes the conditions for activating v11 explicit.

## Section 9 acceptance matrix

| Requirement | M1 decision and delivered evidence | Runtime owner after M1 |
| --- | --- | --- |
| 9.1 Exact calls, descriptors, results, errors, BlockRef and observations | [Wire contract](memory-wire.md), `abi::memory`, fixed-origin model types and [ABI tests](tests/abi/memory_contract/mod.rs); 256-byte input, 128-byte output, exact call matching, zero tails, independent versions/RequestId, 22 error codes | API/Client dispatch and Server descriptor-copy sites, M2 |
| 9.2 Geometry, identity, versions and Narrow | [Reshape contract](memory-wire.md#splitmerge-geometry-and-non-reusing-slot-stamps), `Reshape`, Flat-wide non-reusing stamp issuance, whole-parent bounds and [reshape tests](tests/abi/memory_contract/reshape.rs). Fixed origins never rebase on Narrow | Flat/Pend/allocator atomic commits and all source paths, M2–M4 |
| 9.3 Moves with live publications and PageOwner | First activation rejects any move with an unsettled publication/failed obligation. Published moves and PageOwner adoption are Unsupported; no Keep/Adopt flag or hidden ownership change. Both source and destination need live TRANSFER authority over complete units; maintenance staging retains original authority/blocks | Flat/Page/maintenance integration, M3–M4 |
| 9.4 Trusted topology, exact completion and unsupported boundaries | [Backend contract](memory-backend.md), compiled `entry::boot::memory` interface, non-clonable typed evidence, exact-key combiners, all-refusing default, [evidence tests](../server/tests/memory/evidence.rs) | Metal implementations and platform proofs, M5 |
| 9.5 Fault registration, payload, death and bounded wait | [Fault protocol](memory-wire.md#mapping-fault-routes-and-bounded-recovery), Mapping-owned route, separate route/event counters, fixed-origin event, finite deadline helpers, explicit terminal causes and [fault tests](tests/abi/memory_contract/fault.rs) | Nest, Sign, Task/scheduler and service integration, M3–M4 |
| 9.6 Bounded result recovery and no duplicate execution | [Journal implementation](src/abi/memory/journal.rs), fixed 16 records, actor/request matching, in-flight pinning, immutable terminal results, monotonic eviction boundary and [journal tests](tests/abi/memory_contract/journal.rs) | One live journal per actual Task; resource/result atomic commit, M2–M4 |

The first-version restrictions in rows 9.3/9.4 are explicit decisions, not open
Keep/Adopt or coherence-mode switches. More permissive behavior needs a separate
proof/review; it must not weaken the frozen first-version failure behavior.

## Complete operation boundary

All mutating requests reserve journal capacity before effects. Exact parameter
and result layouts are in the wire contract; the table below completes the
operation-level authorization and linearization inventory.

| Operation | Live authorization and initial state | Linearization / completion and failure |
| --- | --- | --- |
| Flat Create | None FLAT; valid complete logical geometry | Atomically install Fresh range and root grant; no physical materialization; failure exposes no partial root |
| Flat Delete | Full original Flat DELETE and matching FlatVersion | Close admission irreversibly; retain Pending until all holdings/publications/maintenance cleanup is settled, then Closed; failure keeps identity and cleanup |
| Flat Inspect | INSPECT within effective Flat range; Block query also matches its original record/branch access | Snapshot under owner synchronization; no version needed; not permission or proof of content stability |
| Occupy | OCCUPY, matching SlotVersion, Fresh, complete physical unit and compatible block state | Prepare/zero privately, recheck and atomically install one holding/new stamp; loser frees only unpublished preparation; Missing rejects |
| Vacate | VACATE plus original Held binding, matching current Resident Page/version | Atomically leave Missing and release allocation plus maintenance retention only after all exact obligations settle; Pending/Failed retains Page |
| Block | BLOCK over full fixed range; recovery additionally controls the original branch/range and matches old BlockRef | Capacity reserved before closing ordinary admission; Draining remains effective; Held only after required ordinary/overlapping responsibilities settle; death leaves Orphaned |
| Allow | Original live branch, actor and BlockRef | Enter Releasing, forbid new maintenance publication; remove only this block after exact maintenance retirement; never restore revoked grants |
| Push/Pull | Independently authorized complete Flat endpoints with TRANSFER and both Slot versions; original maintenance authority if used | Defined identity lock order, recheck both, atomically move one holding and advance stamps; no live publication/failed obligations in first version; no owner change |
| Split/Merge | RESHAPE over entire parent, expected FlatVersion and matching Held record; exclusive settled allocation receipts | Prepare everything first, atomically convert one level and stamp every changed head; no partial group or Pending conversion |
| Nest Create | None NEST | Atomically create Internal Whole root; no user-selected external requester |
| Nest Delete | Whole DELETE and matching NestVersion | Close space irreversibly, retain exact payload and auxiliary responsibility until Closed; Pending/Failed never permits early reuse |
| Nest Inspect | Whole INSPECT for whole-space state, Mapping INSPECT for the requested Mapping/window; fault queries follow their stronger handler rules | Snapshot/retained query; Whole cannot impersonate a child Mapping grant |
| Reserve | Whole RESERVE and matching NestVersion | Atomically create source-free address reservation, fresh Mapping identity and Mapping Pend; no Page or publication |
| Release | Full original Mapping RELEASE and matching MappingVersion | Enter Releasing; retain address and cleanup until exact retirement then Released; no premature address reuse |
| Insert | Mapping INSERT, additionally REMOVE for replacement; live Flat ASSOCIATE / Share ASSOCIATE / source Memo qualification; target unexposed and settled | Prepare lineage/reverse records, atomically swap logical source under source/target checks and advance generation; failure preserves old source; no implicit Retract |
| Remove | Mapping REMOVE, expected generation, old physical publication settled | Atomically clear logical source and advance generation; no holding transfer |
| Publish | Mapping PUBLISH, current source authority/data ceilings, expected generation, block admission and trusted whole-group proof | Register responsibility before exposure; usable commit only after exact visibility and revalidation; partial hardware failure retains all resources |
| Retract | Mapping RETRACT and exact generation | Retire only this publication; completion needs translation and access-drain evidence; no Page-wide freeze or waiting for unrelated writers |
| Protect | Mapping PROTECT and generation; requested access within all ceilings | Widen repeats admission; narrow keeps old obligation until hardware invalidation; no unaccounted old/new access |
| FaultRoute | Full Mapping FAULT and expected MappingVersion/FaultEpoch; actual service Task and its live Sign grants | Atomically install/replace/disable route, advance both counters; invalidate old waiters; no data grant or handler selected by numeric Task ID |
| InspectResult | Actual calling Task's own request identity, independent of revoked resource grants | Read same stored result; absent IDs below high-water are Expired, future IDs NoRegion; never reexecute an expired request |

Source Memo qualification resolves its immutable original Flat/Share lineage
under Mapping PUBLISH and the underlying association/data ceilings; it cannot
manufacture Flat ownership or broaden rights. Every use rechecks live lineage.
Read/write ceilings, W/X and hardware admission remain separate from operation
bits. Mapping-only FAULT is notification control, not a concurrency permission.

## Why the bounded journal cannot replay effects

`Journal::begin` is the only operation that issues a non-clonable Accepted handle.
For a Task, a fresh ID above high-water obtains a reserved slot before Execute is
returned. Every other path returns InFlight/Replay or an error. Once issued,
high-water never decreases; eviction cannot make that ID fresh again. Skipped IDs
below the mark expire too. A full in-flight window returns no-effect Busy and
does not consume the new sequence. Different canonical bytes under an ID return
RequestMismatch without altering its original record.

`record` validates actor, canonical request and correlated result. Pending cannot
be overwritten with Rejected. Terminal results are immutable except identical
idempotent recording; stale callbacks cannot change Committed to failure. No
Expired pseudo-result is inserted into the journal. Accepted-without-public-result queries return Delivery Busy (no result payload),
never a Rejected receipt or a fresh execution. Those records pin capacity just like Pending, so atomic operations cannot lose their
reservation while preparing. No allocation occurs in admission or recording.

The live integration must enforce these premises: exactly one journal per unique
Task lifetime, one owner synchronization boundary, real kernel actor identity,
only Execute enters mutation, and resource commit plus result recording precede
output copy. Revalidate exact resource/publication epochs separately; the journal
is not their authority. A wrong reply detected by `record` is an implementation
error, not permission to roll back a resource commit. Prepare a valid result and
its reserved storage before commit; record inside the same boundary. Never drop
Accepted as a cancellation instruction. Caller death may destroy journal storage
but resource owners retain all unfinished hardware/maintenance work.

Host tests exercise the actual fixed-capacity mechanism, including lost delivery,
full pending windows, immutable results, cross-domain mismatches, actor isolation,
eviction, skipped IDs, stale handles and u64 exhaustion. This proves protocol
transitions under those premises, not the absent Task/resource integration.

## ABI and boot activation declaration

The selected next version is **ABI 11 / Ledger 11**. It is reserved by this M1
contract, not enabled by this PR. If main allocates 11 for another incompatible
contract before activation, allocate a fresh coordinated version and update this
contract and all consumers together; never combine two meanings under one prefix.

| Surface | Activation change |
| --- | --- |
| Trap call number | Global version 11 in bits 48..63; Flat domain 12 / Nest 13 new slots 16..26; Cell domain 2 retired, no fallback aliases |
| Caller memory | Snapshot exactly 256 request bytes before effects/output, validate actual trap against descriptor, checked addresses, a2..a5 zero; write exactly 128 reply/observation bytes |
| Non-memory domains | Same documented operation slots under global v11; Pend decoders admit only the new explicit rights/scope combinations, not unknown bits |
| Capsule | Keep `SQBOOT01`, format version 2, page shift 12, 32-byte header and 40-byte records; byte 11 becomes ABI 11. Old/new ABI mismatch rejects before execution |
| Ledger | First u32 becomes 11; retain 8-byte header, 40-byte entries and existing reserved-zero checks; no new Slot/Memo/Mapping/Fault target kind |
| Startup names | Space remains Whole Nest; external Nest names remain trusted controller/requester descriptions; Call(Create) remains None license; Page Share views preserve logical source/lineage; Memory and Interrupt retain View/Sign pairing |
| Fault startup | `Trap(PageFault)` keeps its existing Clone diagnostic meaning; it is not a Mapping handler or data grant. Services obtain real Mapping/Flat/Sign grants, register routes, and keep recovery code/buffers resident before clients can fault |
| New rights | Flat OCCUPY bit 6, VACATE bit 7, RESHAPE bit 8; Mapping-only Nest FAULT bit 12. Preserve Whole/Mapping separation and data ceilings; current v10 decoders keep rejecting these additions |
| Backend setup | Trusted boot selects proven providers through Rust interfaces; absence/unknown support rejects new sharing. No user-supplied coherence fact or proof callback |

## Migration order and acceptance gates

1. Merge this contract slice without switching dispatch or version constants.
   Implement M2–M4 in Kernel API/Client/Server: typed call invocation, bounded copy,
   Task journal ownership, live scope checking, logical sources, atomic holds,
   precise retirement/maintenance, bounded faults and retained failure paths.
2. Complete M5 for each enabled platform path. Keep unsupported paths closed.
   Preserve the old provider/image as one rollback unit until new proofs and
   failure tests pass. Exposing the Rust interface does not enable a backend.
3. Publish one coherent Kernel provider revision only after those gates pass.
   The activation commit updates global ABI, Ledger and Capsule expectations
   together, with old-version rejection tests. No mixed v10/v11 memory dispatch.
4. Pin API and Client to that same full published revision in consumers. Vanllia
   platform/image tools, boot grants, shared services, drivers and guest tests
   migrate Cell/Slot paths to Flat/Nest, explicit supply and separate publication.
   Rebuild all Capsules, Ledgers and the assembled image; do not only change a pin.
   Kernel's host fixture currently pins Vanllia at
   `eb37b0b0db299513274864cec23cee509a3cd3cc`; it is not evidence of migrated consumers.
5. Run M6 on the assembled image: cross-Team transfer, independent writable exits,
   Publish/Block/Revoke races, overlapping blocks, reply loss, partial hardware
   failure, service death, swap, shared IPC and Virtqueue. Record host, RV64,
   QEMU, physical-platform and performance evidence separately.

Rollback restores the entire provider, consumer revisions, boot grants and image
set. It must not boot old descriptors under the new prefix or preserve a v11
backend registration inside an old runtime. No consumer pins or images are
changed by this M1 PR.

M1 completion is therefore reviewable from concrete code, tests and decisions.
It does not check off M2–M6, prove a physical platform, activate v11 or close #142.
