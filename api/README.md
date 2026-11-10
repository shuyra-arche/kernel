# Kernel API

`kernel-api` is one `no_std` Cargo package. It owns the public Kernel contracts;
Client executes them and Server implements them. It requires no heap, startup
code or runtime state. There is no Env package or compatibility facade.

## Public modules

| Path | Responsibility |
| --- | --- |
| `address` | PhysAddr, VirtAddr, PageIndex, PageOffset, PageCount and page geometry constants |
| `model::identity` | TaskId, TeamId, PendId and Mark |
| `model::cell` | SlotId, MappingId, allocation, Cell objects, positions, handoffs and observations |
| `model::modifiable` | Modifiable: shared read or exclusive write access |
| `model::unit` | ProgramKind, UnitTarget, control authorities, creation descriptors, queries and stable results |
| `model::{view,pend,clone_view,sign}` | View identity, Pend state, Clone queues and Sign routing |
| `model::{flat,nest}` | Source and address-space identities and observations; never independent authorization |
| `model::{wait,exit}` | Waiting and exit vocabulary |
| `abi::call` | Operation declarations, domain failures and aggregate dispatch |
| `abi::operation` | Operation, RegisterCall, KernelCall and TerminalCall traits |
| `abi::register` | Checked request packing and unpacking, including Wire and Decode |
| `abi::reply` | Shared success-register decoding traits |
| `abi::layout` | Shared little-endian framing for caller-memory descriptors |
| `abi::error` | Common failure representation and dispatch errors |
| `boot` | Initial-task arguments, Capsule, resource Ledger and initrd preamble |

The crate root exposes these four domains directly. Import names from their
owning modules. Each model type owns its checked register and byte codecs.
The Sign operation module owns its success reply and register encoding.

## Operations and generation

`tools/abi-macros` is a separate proc-macro package. Its `envcall` macro generates
only Kernel operation declarations, register codecs, dispatch enums and failure
codes. It is not a service protocol generator. Generated paths refer to
`crate::abi`; API uses this tool at compile time.

A request value contains inputs. `Operation` associates its success and error
types; `RegisterCall` defines its slot and six-word request encoding.
`KernelCall` adds pure return decoding. `TerminalCall` has no return decoder and
cannot be used with a returning invocation. Returning operations use two or
three success registers according to their declaration. Infallible operations
preserve all success bits, including the high bit.

Call classes are 16-bit and operation slots are 32-bit values. The macro checks
register capacity and rejects contradictory declarations. Errors use explicit
consecutive negative codes; an optional busy marker identifies not-ready results.
Unknown call classes, slots and invalid request fields are rejected. Packing and
decoding perform no trap, retry or rollback.

## Boot boundary

Kernel reads the initrd's initial eight-byte offset/length preamble and parses
the selected Capsule. The resource Ledger contains named initial capabilities.
Deployment selection, program catalogs, image packing and service protocols
belong to the integration owner, not this package. The former deployment
manifest parser and packer have been removed.

## Memory geometry: three distinct units

The current public contracts use **4 KiB as their base coordinate**,
not as a universal physical allocation or page-table leaf size.

| Concept | Current representation | Contract |
| --- | --- | --- |
| ABI base page | `address::BASE_PAGE_SIZE` / `BASE_PAGE_SHIFT` (aliases `PAGE_SIZE` / `PAGE_SHIFT`) | `PageCount`, `PageIndex`, `PageOffset`, `CellPosition.page` and Slot/Memo spans count fixed 4 KiB units |
| Managed physical storage | `CellLevel`, size `2^(12 + 9*level)` | Each Cell is contiguous and aligned to its size; separate Cells need not be adjacent. Admission checks the active Sv mode and allocation checks physical capacity |
| Hardware translation leaf | Selected by each MMU/IOMMU backend | RISC-V CPU Sv39 encodes 4 KiB, 2 MiB and 1 GiB leaves; Sv48 adds 512 GiB, Sv57 adds 256 TiB. Automatic coalescing currently stops at 1 GiB. External/IOMMU admits one whole level-0 or level-1 Cell per Memo, with 4 KiB leaves |

A larger PTE leaf can cover multiple logical positions without merging their
identities, authorization, freeze or retirement records. CPU leaf changes
still require splitting and covering TLB completion; external access retains
independent trusted hardware obligations.
`Allocation::at_level(level, count)` requests complete physical Cells.
`Allocation::new(size, align)` remains level 0. Levels admitted by Sv39, Sv48 and
Sv57 are respectively 0–2, 0–3 and 0–4; representability does not imply available
RAM. Slot/Memo positions must name a Cell head; windows and transfers cover whole
Cells. ABI coordinates remain 4 KiB units, so a level-1 Cell spans 512 coordinates.

## Source migration and ABI compatibility

This is a breaking provider redesign of Cell, View, Pend, Sign, Unit and Room.
There are no compatibility aliases for retired operations.

Rust callers match `OutOfMemory` for allocation failures. Each domain uses
its existing numeric ABI code for this variant.

Call numbers are `(version << 48) | (domain << 32) | operation`. The ABI version is **7**.
Other versions are rejected before dispatch.
All byte descriptors use fixed little-endian 64-bit words and checked tags and
reserved fields. Boot Capsule format 2 declares the matching ABI in byte 11;
old executable capsules are rejected before launch. The resource Ledger is
**version 8**, with the same 40-byte entry layout introduced in version 7.

| Domain | Current operations |
| --- | --- |
| Cell | Create (0), Delete (1), Push (2), Pull (3), Publish (4), Retract (5), Protect (6), Inspect (7), Split (8), Merge (9) |
| Flat (12) | Create (0), Inspect (1), Slice (2), Delete (3) |
| Nest (13) | Create (0, Internal only), Inspect (1), Delete (4); slots 2/3 retired |
| Pend | Accord, Revoke, Narrow, Return, Inspect, Collect |
| Clone View | Create, Delete, Push, Pull, Peek |
| Share View | Create, Delete, Open, Shut |
| Sign | Create, Delete, Listen, Trigger, Consume, Attach, Detach, Assign, Resign, Await |
| Unit | Create, Delete, Resume, Freeze, Stop, Exit, Finish, Inspect, Accord, Revoke, Return, Admit, Evict, Profile |
| Room | Yield, SleepUntil, WaitValue, WakeValue, Clock, ClockInfo |
| Debug | Put, Get |

### Migration from ABI v6 to ABI v7

Rebuild executables and regenerate Capsule format 2 with ABI byte 7. The
resource Ledger stays at version 8. `Unit.Exit` uses slot 5 and takes
`(value, reason, note, len, scope)`. Raw callers initialize register `a4` to
0 for Task or 1 for Team. Team creation descriptor word 4 is now the checked
`member_exit` flag. Set it to zero for the existing per-Task exit policy.

### Historical migration from ABI v5 / Ledger v7 to ABI v6 / Ledger v8

| Previous boundary | Current contract | Consumer migration |
| --- | --- | --- |
| Public Port model, rights, calls and Pend target | Removed; domain 14 and target/permission kind 5 remain invalid | Receive a trusted External Nest Pend, not a Port Pend |
| `Nest.Create(license, kind)` | `Nest.Create(license)` creates Internal only | External Nest comes from trusted boot; no user-supplied requester |
| `Nest.Attach(nest, port)` / Detach | Slots 2/3 removed | Use Pend branches for handover and the boot root for terminal Delete |
| Nest ATTACH/DETACH rights | ATTACH remains solely for Internal Nest adoption into Team; DETACH bit 16 invalid | Use ATTACH only for Internal Nest adoption |
| 64-byte NestInfo with PortId | 56 bytes: NestId, kind, phase and four fault words | Resize output buffers and use the checked codec |
| Ledger Port name/class 3, kind 6 | `Name::Nest { controller, requester }`, name class 5, resource kind 7 | Treat topology as descriptive; only the accompanying Pend grants access |
| Per-user Nest creation/binding | One persistent Nest with revocable Pend branches | Retain the manager root; limit operations, Modifiable and Distributable |
| Driver death closes the whole binding | Its exact Memos retire; unrelated branches and NestId survive | Wait for branch cleanup before unsafe address/Page reuse |
| Root termination | Root loss or root-only Nest Delete closes the entire Nest | Busy retains cleanup; retry Delete after settlement; Closed never reopens |
| Capsule ABI 5 / Ledger 7 | Capsule format 2 declaring ABI 6 / Ledger 8 | Rebuild executables, descriptors and ledger readers together |

No new source capability, source Nest or Nest CREATE permission replaces Port.
MMIO/configuration access remains independently authorized. Distribution does not
prove exclusive hardware ownership or device quiescence. Trusted integration
must withdraw old raw device/configuration authority and provide exact platform
ordering evidence before reassigning access; a new software branch is not proof.

### Historical migration from ABI v4 to v5

The geometry changes below remain in v6. Version references describe that earlier
migration, not permission to execute v5 binaries against a v6 Kernel.

| Boundary | ABI v5 change retained in v6 | Consumer migration |
| --- | --- | --- |
| Allocation | Three register words: size, align, level | Supply `CellLevel`; level 0 retains base-page allocation |
| CellCreate Slot | Word 3 carries level; still 96 bytes | Use the new descriptor encoding |
| CellInfo | Word 7 carries level; still 64 bytes | Read physical geometry rather than assuming 4 KiB |
| Flat Create | Fourth argument is level | Pass level explicitly |
| FlatInfo | 32 bytes, including level | Resize output buffers |
| Cell Split / Merge | Operations 8 / 9 return a new SlotId | Replace the old handle only on success |

### Ledger dispatch pairs

Ledger v8 keeps the v7 40-byte entry shape: name `[0,16)`, resource kind at 16,
optional View tag at 17 (0 absent, 1 Clone, 2 Share), zero reserved bytes
`[18,24)`, primary Pend at `[24,32)` and View Pend at `[32,40)`. Handles are
little-endian u64 values. `Name::Interrupt { controller, source }` requires a
Share View and Sign pair; `Entry::dispatch()` returns the explicit View kind
and both handles. Other entry kinds, including Nest, have no paired View.
Malformed, incomplete and old-version ledgers fail validation. Sign operations
retain their slots and payloads under the current ABI prefix.

A source admits one non-distributable Listen/Consume branch only when that Task
holds the matching full writable Share View with Open permission. Listen-only
observers may be derived independently. Initial device inspection and real IRQs
both use ordinary retained sequences and Consume. The source interprets successful
consumption asynchronously; revocation/cancellation does not acknowledge hardware.

### Cell contracts

CellCreate is 96 bytes, CellObject 32, CellPosition 48, CellHandoff 96 and
CellInfo 64. Each request is copied once before decoding. Keep input buffers
readable and output buffers writable through return. Invalid output addresses
fail without creating an output-derived authority. No pointer is retained.
Create returns `(kind, id, address)`: kind 0 is Slot, kind 1 is Memo. SlotId is
Task-local; MappingId needs its CPU Team or I/O target Pend. Neither Page IDs nor
observations confer access. The number zero is invalid for object identities.

| Call | Authority and state | Effect and completion |
| --- | --- | --- |
| Create Slot | None FLAT license; checked allocation geometry | Reserves lazy zeroed Slot capacity; a moved-out Slot never regenerates its Page |
| Split / Merge | Private exclusive complete contiguous Slot collection, adjacent level, common owner and allocator, target alignment; no bindings, borrows or hardware obligations | Repartition allocator receipts without copying data; new Page identities and SlotId; old handle consumed only on success; failed preparation changes nothing |
| Create Memo | Authorized Slot window or relative Pend window; destination control | Reserves address positions; Slot source starts Vacant, Pend source supplies Ready associations; never publishes |
| Delete Slot | Calling Task's SlotId | Releases that owning handle; Memo and hardware responsibilities survive |
| Delete Memo | Mapping controller, explicitly permitted Space members, or authorized I/O target controller | Requires all publication withdrawn and fenced; deletes associations and freezes; Busy preserves the record |
| Push / Pull | Source access plus destination control; actual Slot ownership for any owning transfer | Same endpoint matrix, expressed from giver/taker; no PTE or IOMMU publication effect |
| Publish | Valid source authority and qualified Ready Memo | Starts hardware publication of the associated Page, not whatever now occupies its source Slot; Inspect distinguishes Publishing from Published |
| Retract | Qualified Published Memo (I/O also Publishing) and mapping control | Closes Page admission before withdrawal; accepted I/O request is not completion; query the original object |
| Protect | Qualified CPU Memo and access within its original ceiling | Changes one position's access and generation; no new association or ownership; External Protect is unsupported; create a replacement Memo with different permissions |
| Inspect | Owning Slot or mapping observation authority | Non-consuming state snapshot, including during external access; no raw physical address or reusable permission |

| Source → destination | Effect |
| --- | --- |
| Slot → Slot | Atomically moves the unique holding into an empty Slot; occupied, pinned or external endpoints fail |
| Slot → Memo | Associates the Page with the authorized source window; target must be unpublished |
| Memo → Memo | Copies an authorized association; compatible source windows and both generations are checked |
| Memo → Slot | Moves the actual controlled source Slot only if it still contains this exact Page; borrowed Memos never mint ownership |

Memo identity survives replacement. Its stable states are Vacant, Ready(Page)
and Published(Page); Publishing/Retracting are retained flights, Failed retains
an unresolved hardware error. Association, publication, retraction and protection
advance a non-wrapping generation. Use Inspect to refresh after Busy; replaying
an old generation never starts a later cycle. Slot positions use generation zero
and act on the current holding, not on an alleged historical content snapshot.

Retract retains a persistent freeze in the Memo. `frozen` proves the admission
gate is closed, no writable hardware responsibility remains, and the Page is
not quarantined. A sequence check alone proves none of this. Another frozen
Memo blocks a conflicting writer even through another authorized publication;
explicit read-only publication may preserve a frozen snapshot. Publish,
association replacement or Delete ends that object's freeze. Failed publication
may advance the generation and leave Ready with its freeze, so retry from Inspect.
Historical observation references do not prolong deleted freezes after fencing.

Retract completion settles only this mapping. `external` and other retained
CPU/device obligations may still forbid moving or reclaiming the Page. Physical
reuse requires all such obligations to settle, independently of business results,
Sign notifications, timeouts, handle deletion or Task exit. Ready is not a claim
that memory has been freed. Task death and creating-branch revocation retain
hardware records through completion. Their orphaned Memos, including unused
reservations, are then removed; later lookup of the old MappingId fails.

External Memos use one complete level-0 or level-1 Cell, aligned to its size,
Caller control and READ or READ|WRITE. Overlapping ranges are rejected.
One binding supports up to 512 base-page Cells or one level-1 Cell within the
aligned 2 MiB window selected by its first Memo. Positions outside that window
are rejected. Pend handover does not reset this hardware window. A settled Memo
can be reassociated and republished; retracting one leaf leaves other leaves
accessible. Page substitution still requires an explicit Cell Push/Pull
association and a fresh generation from Inspect. Each range retirement requires
matching invalidation/fence completion and trusted platform ordering. Binding
retirement is a separate, whole-context boundary; it is not performed on every
Pend handover. Unsupported aliases, shared controllers, ATS/PRI and noncoherent
access are rejected.

Public Acquire/Release, Attach/Detach, Begin/End, Traverse, Position, Attachment
and general Retain authority are removed. Their replacements are Cell objects,
qualified transitions and kernel-owned hardware retirement, not a public change
receipt or duplicate reference lifecycle.

Split/Merge operate on owning Slot collections, not PTE leaves. Flat/Share roots
are not eligible. Every source Slot must be materialized; merge requires physical
adjacency. New metadata and the replacement handle are prepared before receipt
commit, so allocation failure preserves the original collection.

### Runtime organizations

Flat Create takes `(license, size, align, level)` and returns a Flat-target root Pend;
it reserves lazy Slots using the frame allocator. Flat Inspect writes 32 bytes
(FlatId, effective page offset, count, level). Flat Slice returns a Task-local SlotId for a relative window;
that Slot retains the exact live Pend, not a copied permission snapshot. Every
use rechecks the current branch/window. It cannot bypass Narrow, suspension or
Revoke. Flat Delete seals the source and withdraws associations; Busy keeps cleanup
responsibility. Cell Delete of a Slot releases only that handle.

Share Create on a Flat Slice requires None SHARE, Flat SHARE, Distributable Yes
and a permitted borrow. It creates a Share child in the same Pend tree. Flat
ASSOCIATE permits Share OPEN/SHUT/ASSOCIATE; Flat DELETE alone permits the child's
DELETE right. Generic Accord never changes target kind. Flat TRANSFER is checked
separately at both ends of Cell Push/Pull into a Slot; it cannot be inferred from
read/write access. A mutable parent is suspended while its data is lent.

Nest Create takes a None NEST license and returns an Internal root. External
Nests are created only by trusted boot admission, which authenticates the complete
controller/requester isolation boundary before delivering an ordinary Nest Pend.
The Ledger name describes that topology; it is not accepted as a runtime credential.
Nest Inspect writes 56 bytes with stable NestId, kind, phase and retained fault
information. Public Port identities, rights, calls and a second authorization
lineage do not exist.

Keep the External Nest root with its manager. Accord delegates a subset of
Nest operations, Modifiable and Distributable to workers. A worker cannot widen
its rights or acquire another source by supplying a raw number. Its death,
Return or Revoke invalidates the branch and retires its Memos, not the entire
Nest. The exact creating branch remains retained until its positions are idle
and removed; a live publication additionally retains its actual publishing
permission and data borrow. Other branches of the same Task keep their live
leaves. Only one Task at a time may reserve positions in an External Nest.
A competing reservation returns Busy until the former Task's positions have
all been removed. Reusing an old address requires hardware settlement, and
the replacement Memo receives a different identity.

Root loss, Delete by the External Nest root with DELETE permission, or an unsafe backend failure
closes the entire address space. Delete starts irreversible Closing and returns
Busy while binding retirement or cleanup remains; retry completes deletion.
Closed cannot reopen. A private Unbound state exists only during trusted boot
preparation, never as a user opportunity to attach arbitrary hardware. The
controller stays assigned until the backend's exact complete binding and platform
proof; failure retains Pages, table pages and queues. No temporary driver Task
is the sole owner responsible for completing this cleanup.

Sign Attach includes `generation`. MappingPublished (tag 11) and MappingRetired
(tag 9) observe the exact Memo generation, including completion before registration.
The immediate next generation may be armed before publication. An older generation
reports Gone rather than matching a replacement publication. NestClosed (13) and
NestFault (12) require generation zero and authorized Nest INSPECT. Hardware FQ
records have no software Memo generation: the oldest fault is retained at Nest
scope, subsequent faults increment a saturating sequence, and no unbounded queue
or guessed Memo attribution is exposed. Listen/Consume and Pend handover do not
clear Nest faults. MappingFault (tag 10) is retired. Whole-Nest close and ordinary
branch retirement remain observably different.

| Operation | Required permission | Additional boundary |
| --- | --- | --- |
| Flat Create / legacy Slot allocation | None FLAT | Checked allocation geometry |
| Flat Inspect / Slice / Delete | Flat INSPECT / ASSOCIATE / DELETE | Current window; Delete retains failed withdrawal |
| Cell Push/Pull to a Flat Slot | Flat TRANSFER on each Flat endpoint | ASSOCIATE handle, Modifiable Yes, movable Page |
| Share Create from Flat | None SHARE + Flat SHARE | Distributable, narrowing and borrow rules |
| Nest Create | None NEST | Internal only; no hardware source parameter |
| Team Create with Internal Nest | Build + Nest ATTACH | Exclusive owned root; token consumed |
| Nest Inspect / Delete | Nest INSPECT / DELETE | External Delete requires the boot root; Busy retains cleanup |
| External Memo Create / Delete | Nest RESERVE / RELEASE | Source authority; exact supported position |
| External association / Publish / Retract | Nest TRANSFER / PUBLISH / RETRACT | Exact Memo generation and source access |
| External Memo Inspect / Sign conditions | Nest INSPECT | Same Nest; retained state |
| External Protect | Unsupported | Reject; CPU Protect remains existing Team/source authority |

Nest PROTECT is reserved and has no External implementation. None grants no
external source creation, raw hardware access or arbitrary syscall permission.
For Internal Nest, RESERVE/RELEASE/etc. do not create a second mapping API: once
adopted by a Team, the existing Team/Cell contract controls its CPU Memos.

### Authority and control contracts

Pend Accord snapshots an 88-byte request; Narrow snapshots a 64-byte request.
Both carry typed operation permissions, independent Modifiable/Distributable
limits, and a checked range. Derivation requires Distributable; neither
derivation nor narrowing can add operations or change the target family.
Both Accord and Narrow interpret a Pages range relative to the immediate
parent's/current effective window. Narrow retains identity, rejects live
descendants with Busy and withdraws old CPU access before committing a smaller
window. Active data borrows and publication routes retain their constraints.
Retirement-only holds keep cleanup responsible without preserving permission or
permanently preventing Narrow. The 112-byte PendInfo identifies its target and
includes effective permissions and source window. FlatId and NestId are stable
object identities; they grant no access on their own.
None, Sign, Flat and Nest use the same live Pend ledger and derivation tree
as Views. No descriptive identity grants access. None is object-free call permission: its Modifiable
must be No, its range Inherit, and it permits only the listed CreationRights.
The Kernel mints the initial None root for the bootstrap Task in Ledger
`Name::Call(Call::Create)`. A new Task receives no implicit creation permission;
its manager must explicitly Accord a subset before it invokes creation calls.
None and Sign accept no page range. Sign also normalizes Modifiable to No;
LISTEN/TRIGGER/CONSUME/ROUTE/DELETE control behavior without claiming data access.
Operation bits are typed by target and mapped to the fixed ABI operations below;
the caller cannot supply a different target or operation identity to bypass them.

| Live View operation | Required operation permission | Additional condition |
| --- | --- | --- |
| Clone Push | Clone PUSH | Modifiable Yes; queue capacity and payload bounds |
| Clone Pull | Clone PULL | A live queued message; payload becomes an independent read-only Share |
| Clone Peek / queue condition | Clone PEEK | Live queue |
| Clone/Share Delete | Matching DELETE | Live branch; closing Share can retry pending withdrawal |
| Share Open | Share OPEN | Effective range, Modifiable and borrow constraints |
| Share Shut | Share SHUT | Matching live branch and mapping |
| Cell association through Share | Share ASSOCIATE | Same branch/window, Page and publication constraints |

Creation grants all supported operations to the new View's root. Boot View
grants are narrower: fault queues allow PULL/PEEK, memory resources allow
OPEN/SHUT/ASSOCIATE. Neither permits deleting Kernel-owned Views. The trusted
External Nest root is separately delivered to its manager, including the ability
to terminate that Nest; worker branches should omit DELETE. Accord requires an
explicit operation subset and Distributable Yes. Return and Revoke continue to
follow ownership of the authorization branch and need no extra operation grant.
Removing data permissions first withdraws existing mappings; retained Slot
references revalidate the same authority before association. Inspect/Collect
return identity and state, never a reusable authorization proof. Return and
Revoke complete only after descendant and retained hardware responsibilities
have settled. Busy retains cleanup responsibility.

Wait has exactly Poll, At(monotonic nanoseconds), and Infinite. Deadlines are
converted once, rounded up, and retained across wake/requeue. WaitValue compares
and enqueues under the same boundary as WakeValue; callers keep the AtomicU32
and mapping alive through all waits. True requires rechecking the predicate.

Sign retains one event with a non-wrapping sequence. Listen observes;
Consume(sequence) rejects stale sequences and only clears that direct event.
It does not acknowledge an IRQ, consume a Unit result or resolve a page fault.
Attach watches authorized sources. Pend Accord reduces operation rights and
Distributable; Pend Revoke/Return retire the entire branch and wake listeners.
A listener racing retirement may observe Dead before table removal or Denied
after removal. Neither result permits further access. SignId names the endpoint
only; all Sign calls and SignSource::Sign take a Task-local PendId. Delete requires
DELETE and closes the endpoint for every holder. Returning a branch revokes its
descendants without closing another live branch's endpoint. Owner death closes
owned endpoints. Routes belong to their destination Pend; removing ROUTE or
returning that Pend removes its routes and preserves other branches' routes.
Direct event sequences and public authority IDs never wrap or revive stale uses.

Unit Create snapshots a 48-byte Team or Task descriptor. Tasks start dormant.
Team creation requires Build authority. Its optional `nest: PendId` (word 3, zero
means allocate a new Nest) transfers an initialized Internal Nest into the new
Team. The caller must hold its root Nest ATTACH Pend with Modifiable Yes, no
children and no hardware holds. Success consumes the root token; failure does
not publish a Team. External Nest, delegated roots, kind mismatch and repeated
adoption are rejected. Later CPU Cell operations use the existing Team manager
qualification. This does not attach an External Nest to CPU execution.
Supervisor creation additionally requires
a Supervisor caller. A Guard authority may choose an immutable fail-stop policy
at Team creation, before any member can execute; dynamic Guard/Unguard are gone.
Membership alone does not authorize managing peer Tasks. Resume/Freeze/Stop
require the controller or ancestor Team manager; Stop is a request, not proof of
completion. Finish permanently closes member admission.

Unit Inspect snapshots identity, state, result and independent reclamation status
into 112 bytes. Caller, target, child enumeration and member enumeration share
this query contract. Observing a result does not consume it. Task Delete requires
completed reclamation and removes only the caller's retained record; subsequent
observations through that record are denied. Team Delete requires retirement.
Task scope publishes the calling Task's value and a bounded diagnostic note.
Team scope also commits a result for the caller's Team, closes its admission,
and queues termination of the remaining members and descendants through the
same sweep as Stop. Team scope requires a non-realtime Task and a Team created
with `member_exit` enabled. Boot Teams and guarded Teams use Task scope.
An authorized manager can read the Team result with Inspect, independently of
member completion and reclamation. The first committed termination wins.
Unit Accord/Revoke/Return operate on independent administrative authorities with
revocable ancestry.

| Call | Required authority / state | Completion or retained effect |
| --- | --- | --- |
| Clone Create | None CLONE license | New queue and root Pend |
| Share Create | None SHARE license and authorized Slot/source window | New View root or narrower branch; source qualification and borrowing still apply |
| Clone/Share Delete | Matching target DELETE | Closes object; unfinished borrows and hardware obligations remain |
| Clone Push | Clone PUSH and Modifiable Yes; queue limits | Enqueues one copied message or fails; no implicit retry |
| Clone Pull / Peek | Clone PULL / PEEK | Pull delivers message authority; Peek observes length/sender/count without consuming |
| Share Open / Shut | Share OPEN / SHUT; range, Modifiable and borrowing | Opens/closes this caller's access using the common Nest publication machinery; Shut neither returns Pend nor deletes View |
| Pend Accord | Delegable parent and non-expanding relative window/access | New local/remote branch; failed preparation does not publish partial authority |
| Pend Revoke / Return | Direct grantor / holder | Retires branch; Busy keeps obligations and parent data access suspended where lent |
| Pend Narrow | Holder; no live descendants, conflicting data borrow or active publication | Same identity with reduced access/window; retirement-only retention is not live permission |
| Pend Inspect / Collect | Caller-local table | Single/paged discovery; stable View identity allows comparison, never authorization |
| Sign Create | None SIGN license | New endpoint and root Sign Pend |
| Sign Delete | Sign DELETE | Closes endpoint and wakes all listeners |
| Sign Listen | Sign LISTEN; Poll/At/Infinite | Observes direct event or routed source; does not consume either |
| Sign Trigger / Consume | Sign TRIGGER / CONSUME; empty/current sequence respectively | Sets a new sequenced event / clears exactly that event; stale consume fails |
| Sign Attach / Detach | Sign ROUTE plus source observation authority | Adds/removes a route; readiness is revalidated while waiting |
| Unit Create | Own Team or controlled constructing child; Build for Team | Dormant Task or empty Team; explicit Resume begins execution |
| Unit Delete | Authorized observer/manager and completed reclamation | Removes caller's Task record or retired Team; unfinished cleanup is not discarded |
| Unit Resume / Freeze | Controller or ancestor manager | Admits/suspends execution; does not alter results or transfer authority |
| Unit Stop / Exit | Controller / executing Task with scope-specific authority | Requests termination / exits the Task or its opted-in Team with a result |
| Unit Finish | Team management | Permanently closes member admission |
| Unit Inspect | Caller identity or authorized target/enumeration | Stable non-consuming result/state/reclamation observation; expired records fail |
| Unit Accord / Revoke / Return | Administrative qualification with allowed delegation / direct grantor / holder | Changes administrative ancestry, never data borrowing or membership |
| Room Yield | Executing Task | Relinquishes current execution opportunity |
| Room SleepUntil | Executing Task; absolute monotonic ns | Resumes at/after the deadline; earlier wakes do not extend it |
| Room WaitValue / WakeValue | Aligned accessible AtomicU32 in same address space | Atomic compare/enqueue or wake current waiters; no stored notification, recheck predicate |
| Room Clock | Executing Task | Full-width monotonic ns observation |
| Room ClockInfo | Direct counter authorized on all admitted Harts | Success returns status zero, boot tick and frequency in a0–a2. Fall back to Clock if unavailable. |
| Debug Put / Get | Valid bounded caller buffer | Bounded diagnostic I/O, reports transferred length |

Failures do not imply rollback of accepted asynchronous retirement. Query the
original object's state and generation before deciding whether to retry. Invalid
identities, missing authority and stale branch rights are checked inside Kernel;
Client never substitutes prior observations for these checks.

Dedicated Join/ReadResult, SelfId/Sire/Creator/Scan, generic key Wait/Wake,
Chrono/Ticks, Control/Backtrace and Debug SetTrace have been removed. Inspect,
Sign routing and Room Clock provide their retained core capabilities. Kernel
crash backtraces and bounded Debug Put/Get remain available.

Consumers must rebuild together with this API and regenerate capsules/ledgers.
Provider and consumer revisions must move together. The following historical
migration describes changes retained from ABI v4; current binaries must use v7.

| Previous interface | ABI v4 / Ledger v5 change | Consumer migration |
| --- | --- | --- |
| Clone/Share Create without permission | Leading `license: PendId` with None CLONE/SHARE | Read boot Create license or receive it through Pend Accord |
| Sign Create returning SignId | None SIGN license; returns PendId | Use Pend Inspect for descriptive SignId |
| Sign calls and SignSource::Sign using SignId | Same operations with Sign-target PendId | Resolve only the caller's actual Pend |
| Sign Accord/Return/Revoke at slots 7–9 | Removed; use Pend Accord/Return/Revoke | Encode Sign operations in Limits; do not reinterpret old handles |
| Cell Create | Leading license; Slot requires None FLAT | Supply explicit license for allocation; Memo authority comes from source/destination |
| Separate Sign table | One Task Pend ledger | Collect includes None/Sign; inspect Target before using a token |
| Pend descriptors in v1 | Target-tagged Limits, 88-byte Accord, 64-byte Narrow, 112-byte Info | Rebuild byte snapshots; unknown bits and mismatched target tags reject |
| Boot interrupt handle in Ledger v3 | Sign-target Pend in Ledger v5 | Rebuild capsule ABI byte and ledger parser; cannot cast an old Sign handle |

Room operations and bounded Debug Put/Get are inherent to the executing Task,
subject to address access and counter capability checks. They need no None grant.
Unit controls remain ControlId-based: Build/Guard and target/controller checks do
not become creation licenses. Pend Inspect/Collect and Return are holder actions;
Revoke is a direct-grantor action, avoiding recursive permission to surrender
permission. Busy never removes a retained hardware obligation. This explicitly
supersedes #14's former View-only Pend definition as recorded in #84.

## Verification

From the repository root:

```sh
cargo test --locked -p kernel-api --tests --target x86_64-unknown-linux-gnu
cargo test --locked -p kernel-api-macros --test declaration --target x86_64-unknown-linux-gnu
```

The `tests/abi/mod.rs` and `tests/boot/mod.rs` suites group behavior by domain.
Cargo declares these test entry points explicitly. Executable examples and
compile-fail fixtures live under `tests/abi/`; library doctests are disabled. Host
checks protect published slots, encoding, malformed input rejection and typed
operation boundaries. They do not execute traps. See the root README for RV64
builds and the separate responsibility for system integration.

### Earlier external-interface migration

The Port-based intermediate contract from #83/#84 is superseded by the ABI v6
migration above. The following independent changes remain in force:

| Previous contract | Current replacement | Required migration |
| --- | --- | --- |
| External authority selected through BAR Share | Trusted External Nest Pend, independent of MMIO Share | Read the Nest grant and delegate existing Pend branches |
| One-shot single-page external route | Repeatable Memo positions under one persistent Nest | Retract only the leaf; Delete only to terminate the entire Nest |
| Sign Attach without generation; MappingFault | Generation-qualified MappingPublished/Retired and NestFault | Pass generation explicitly, use zero for non-Memo sources |
| Team descriptor word 3 reserved zero | Optional owned Internal Nest root | Existing callers pass None; adopting callers consume root on success |
| No runtime Flat/Nest calls | Classes 12/13 | Rebuild API, Client, executable and Capsule ABI byte together |
| MMIO/aperture coupled to requester | `grant_mmio` plus independent trusted ExternalRoute controller/requester/proof | Platform establishes authentic routes before publishing Ledger grants |
| Quiescence poll for whole shutdown | `prove(Retirement)` with Range/Binding scope | Exact-token proof; range must drain prior requests without closing other live leaves |

The initial coherent RV64/QEMU implementation admits one trusted requester per
controller and rejects shared/aliased/unsupported isolation. The integration
owner controls configuration that could bypass isolation. Kernel performs no
PCI enumeration, BAR allocation, device business control or Board parsing.
Kernel-only provider changes do not migrate consumers automatically. Record full
provider and fixture revisions when validating integration. No physical-device
proof follows from Host tests or the coherent QEMU fixture.

Debug Put is nonblocking and returns the exact accepted byte prefix (possibly zero).
It adds no newline and does not rewrite non-UTF-8 bytes. Callers retaining output
must retry the unaccepted suffix; Kernel never reports discarded bytes as written.

## Trusted boot provider input

The source-level boot contract is separate from the versioned syscall and boot
wire formats. `boot::platform::{Memory, Region, Resources}` admits bounded
physical RAM, reservations, opaque firmware metadata and MMIO custody. Common
admission knows no Sv mode, SBI target, Sstc flag or PLIC context. `Resources`
validation does not grant authority or prove a hardware route is supported.

The complete implemented input lives at `boot::platform::riscv`: `Platform`,
`ClockAccess`, `hart::{Cpu, Harts, HartId, FirmwareId, Paging, HartError}` and
`devices::{Devices, Plic}`. The selected metal provider chooses this input for
Kernel entry. Firmware declarations, verified capabilities and Kernel controller
ownership remain distinct: existing CPU checks/probes and RV64 device validation
still precede publication. There is no generic ISA enum or second-ISA boot claim.

| Previous source | Replacement |
| --- | --- |
| `platform::{Platform, ClockAccess, Cpu, Harts, HartId, FirmwareId, Paging}` | The same names under `platform::riscv` |
| `platform::hart::HartError` | `platform::riscv::hart::HartError` |
| `platform::devices::{Devices, Plic}` | `platform::riscv::devices::{Devices, Plic}` |
| `Devices.{authorities, protected, external}` | `Devices.resources: platform::Resources` |
| `Memory::dtb()` | `Memory::metadata()`; the constructor's fourth argument is opaque firmware storage |

`Platform::dtb()` remains RV64-specific. Physical range units, pre-heap capacities
and separate static memory/Hart storage are unchanged. Images must rebuild against
the source contract and the ABI/Ledger versions above. The Host second-provider
fixture validates common physical inputs without manufacturing RISC-V facts;
CPU/controller admission for another ISA awaits an actual provider.

`boot::bind_external(ExternalRoute { controller, requester, proof })` is an unsafe
trusted Rust entry, not a Unit syscall. It allocates the External Nest and binds
the validated source before Ledger publication. Its signature is unchanged, but
its output grant is now the Nest root. Ordinary Unit creation cannot authenticate
or select a hardware source. Failed boot cannot publish partial authorities.

## Optional CPU reservations and explicit service waits

Unit slots 11/12/13 are `Admit(task, schedule)`, `Evict(task)` and
`Profile(task, out)`. Schedule is seven little-endian u64 words: Hart slot,
base priority, inheritance ceiling, budget ns, period ns, latency assumption ns,
and extension flags (bit 0 F/D; bit 1 V including F/D). ScheduleInfo appends six
u64 words: effective priority, remaining ns, next replenishment uptime ns,
consumed ns, overruns ns and conditional response ns (104 bytes total).

Sign slots 10/11/12 are `Assign(token, server, grant)`, `Resign(token)` and
`Await(token, wait)`. Slots 7–9 remain retired. Await has the same Wait and
three-register reply encoding as Listen, with an explicit live server dependency.
These operations were introduced in ABI v5; their slots and payload layouts are
unchanged under the current ABI prefix.

Admit requires Build and Task management authority and a stopped User Task;
Evict requires the same authority and an initially dormant Task; Profile requires
management authority. Assign requires ROUTE plus control of the admitted server
and its live Trigger grant to the same direct endpoint. Await uses LISTEN and
requires caller/server reservations on the same Hart. Invalid identity, policy,
cycle/depth or ceiling escalation is Denied; unschedulable admission is Busy;
metadata/extension allocation failure is OutOfMemory. Rebinding invalidates old
waits; server death is Dead and revoked authority is Denied.

The [execution contract](../server/tests/realtime/README.md) specifies all ranges,
resource preparation, allowed operations, separate server budgets, accounting,
response analysis assumptions and reproducible validation. In particular,
Profile.response covers conditional runnable CPU service, not arbitrary external
service completion. Unit Exit from this class retains result/reason but ignores
the diagnostic note; safe physical reclamation remains asynchronous.
