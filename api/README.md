# Kernel API

`kernel-api` is one `no_std` Cargo package. It owns the public Kernel contracts;
Client executes them and Server implements them. It requires no heap, startup
code or runtime state. There is no Env package or compatibility facade.

The [Reservation migration](memory-reservation.md) implements the same-round
address-reservation naming change from #142 section 14. This changes Rust source
names without changing the running v10 wire layout or activating ABI 11.

## Public modules

| Path | Responsibility |
| --- | --- |
| `address` | PhysAddr, VirtAddr, PageIndex, PageOffset, PageCount and page geometry constants |
| `model::identity` | TaskId, TeamId, PendId and Mark |
| `model::cell` | SlotId, allocation, Cell objects, positions, handoffs and observations |
| `model::modifiable` | Modifiable: write permission on a Pend |
| `model::unit` | User-only creation, UnitTarget, control authorities, creation descriptors, queries and stable results |
| `model::{view,pend,clone_view,sign}` | View identity, Pend state, Clone queues and Sign routing |
| `model::memory` | Bounded, atomic reads of versioned global memory observations |
| `model::{flat,nest}` | Source, Nest and ReservationId identities and observations; never independent authorization |
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
| ABI base page | `address::BASE_PAGE_SIZE` / `BASE_PAGE_SHIFT` (aliases `PAGE_SIZE` / `PAGE_SHIFT`) | `PageCount`, `PageIndex`, `PageOffset`, `CellPosition.page` and Slot/mapping spans count fixed 4 KiB units |
| Managed physical storage | `CellLevel`, size `2^(12 + 9*level)` | Each Cell is contiguous and aligned to its size; separate Cells need not be adjacent. Admission checks the active Sv mode and allocation checks physical capacity |
| Hardware translation leaf | Selected by each MMU/IOMMU backend | RISC-V CPU Sv39 encodes 4 KiB, 2 MiB and 1 GiB leaves; Sv48 adds 512 GiB, Sv57 adds 256 TiB. Automatic coalescing currently stops at 1 GiB. External/IOMMU admits one whole level-0 or level-1 Cell per mapping, with 4 KiB leaves |

A larger PTE leaf can cover multiple logical positions without merging their
identities, authorization, freeze or retirement records. CPU leaf changes
still require splitting and covering TLB completion; external access retains
independent trusted hardware obligations.
`Allocation::at_level(level, count)` requests complete physical Cells.
`Allocation::new(size, align)` remains level 0. Levels admitted by Sv39, Sv48 and
Sv57 are respectively 0–2, 0–3 and 0–4; representability does not imply available
RAM. Slot/mapping positions must name a Cell head; windows and transfers cover whole
Cells. ABI coordinates remain 4 KiB units, so a level-1 Cell spans 512 coordinates.

## Source migration and ABI compatibility

The [next Flat/Nest memory contract](memory.md) tracks #142 against the current
ABI v10 baseline. Its fixed-origin position and revision codecs are available in
`model::{flat,nest}` for migration preparation; current calls do not accept these
new descriptors. Runtime Occupy/Vacate, maintenance BlockRef and the next ABI
remain unimplemented. This does not change the current contracts below.
The inactive `abi::memory` module provides [bounded request/result codecs](memory-wire.md)
for next-version transitions (including Split/Merge) and object lifecycles,
including exact status codes,
result inspection, scoped Flat/Slot/Block/Nest/Memo/Reservation observations, and
Reservation fault route/event descriptors with checked finite deadlines.
The [M1 acceptance record](memory-m1.md) also covers the bounded request journal
and trusted backend proof interface. The [M2 acceptance record](memory-m2.md)
adds scoped Flat/Nest calls, a checked register Frame and Client invocation.
These are not accepted by current `EnvCall`; live v11 integration remains an
activation gate, and the existing v10 invocation traits stay unchanged.
The [M3 progress record](memory-m3.md) tracks the initial explicit-residency
provider and Task journal integration; logical Memo/publication migration and
maintenance-backed Vacate remain unfinished.

This is a breaking provider redesign of Cell, View, Pend, Sign, Unit and Room.
There are no compatibility aliases for retired operations.

Rust callers match `OutOfMemory` for allocation failures. Each domain uses
its existing numeric ABI code for this variant.

Call numbers are `(version << 48) | (domain << 32) | operation`. The ABI version is **10**.
Other versions are rejected before dispatch.
All byte descriptors use fixed little-endian 64-bit words and checked tags and
reserved fields. Boot Capsule format 2 declares the matching ABI in byte 11;
old executable capsules are rejected before launch. The resource Ledger is
**version 10**, with the same 40-byte entry layout introduced in version 7.

| Domain | Current operations |
| --- | --- |
| Cell | Create (0), Delete (1), Push (2), Pull (3), Publish (4), Retract (5), Protect (6), Inspect (7), Split (8), Merge (9), Insert (10), Remove (11) |
| Flat (12) | Create (0), Inspect (1), Slice (2), Delete (3), Block (4), Allow (5) |
| Nest (13) | Create (0, Internal only), Inspect (1), Delete (4), Reserve (5), Release (6); slots 2/3 retired |
| Pend | Accord, Revoke, Narrow, Return, Inspect, Collect |
| Clone View | Create, Delete, Push, Pull, Peek |
| Share View | Create, Delete, Open, Shut |
| Sign | Create, Delete, Listen, Trigger, Consume, Attach, Detach, Assign, Resign, Await |
| Unit | Create, Delete, Resume, Freeze, Stop, Exit, Finish, Inspect, Accord, Revoke, Return, Admit, Evict, Profile |
| Room | Yield, SleepUntil, WaitValue, WakeValue, Clock, ClockInfo, Inspect |
| Debug | Put, Get |

### Migration from ABI v9 / Ledger v9 to ABI v10 / Ledger v10

Rebuild API, Client, Server and all executables together. Regenerate Capsule
format 2 with ABI byte **10** and Ledger version **10**. Old and future versions
are rejected; retired target/operation kinds 5 (Port) and 7 (Memo) remain invalid.
There are no aliases for MemoId, MemoRights, MappingTarget, CellCreate,
CellSource, MemoDestination or CreatedCell.

| ABI 9 boundary | ABI 10 boundary | Consumer action |
| --- | --- | --- |
| Memo target and operation kind 7 | Nest target kind 4, with typed Whole or Reservation scope | Delegate a live scoped Nest Pend; never authorize from a ReservationId or TeamId |
| `Limits.range` | `Limits.scope: PendScope` | Use Object for None/Clone/Sign, Pages for Flat/Share, Nest for Nest operations |
| 88-byte Accord / 64-byte Narrow / 112-byte PendInfo | 104 / 80 / 128 bytes | Resize copied descriptors and observation buffers |
| Cell Create descriptor / three-register CreatedCell | `Create { license, allocation: Allocation } -> SlotId` | Pass allocation's three words in registers; create a mapping separately with Nest Reserve |
| Cell Delete object pointer | `Delete { slot: SlotId } -> ()` | Delete only owning Slots; use Nest Release to retire a mapping |
| 32-byte CellObject / 48-byte position / 96-byte pairs | 16 / 32 / 64 bytes | Encode Slot or Pend directly; Push/Pull accept only two Slots |
| Team-selected CPU destination | Whole-scoped Nest RESERVE | Hold or receive explicit CPU Space authority; membership and management are not mapping permission |
| Unit Create returns UnitTarget in two registers | CreatedUnit in three registers | Retain Team's returned Nest Pend; Task replies carry no grant |
| No current CPU Ledger entry | `Name::Space`, name class 7/item 0, ResourceKind::Nest | Inspect the live grant; boot grants have Whole INSPECT\|RESERVE only |

#### Scoped permission layout

All descriptor words below are little-endian u64. A Scope is exactly five words
`[tag, id, range_tag, offset, count]`:

| Tag | Typed scope | Canonical payload |
| --- | --- | --- |
| 0 | `PendScope::Object` | `[0, 0, 0, 0]` |
| 1 | `PendScope::Pages(range)` | `id = 0`, then PendRange |
| 2 | `PendScope::Nest(NestScope::Whole)` | `[0, 0, 0, 0]` |
| 3 | `PendScope::Nest(NestScope::Reservation { id, range })` | Nonzero ReservationId at most isize::MAX, then PendRange |

PendRange is `[0, 0, 0]` for Inherit or `[1, offset, count]` for Pages.
Pages are nonempty and both coordinate addition and byte span must fit. Unknown
tags, reserved payload, invalid IDs and overflow reject. Inherit is accepted in
requests only; published ranged PendInfo contains resolved **absolute Pages**.
The server resolves requested relative ranges against the parent's/current
window and checks bounds at the authorization commit. Pure Limits attenuation
checks do not resolve those relative bounds.

Limits is nine words (72 bytes): `[operation_kind, operation_bits, modifiable,
distributable, scope_tag, scope_id, range_tag, offset, count]`. None/Sign require
Modifiable No; Distributable is independent. AccordRequest is 13 words (104 bytes):
`[src, recipient_tag, recipient_id, mark, Limits...]`. Recipient tag 0 has ID zero;
tag 1 names a Task. NarrowRequest is ten words (80 bytes): `[token, Limits...]`.
PendInfo is 16 words (128 bytes): `[token, target_kind, target_id, owner, lender,
mark, status, Limits...]`. Decoding checks both target family and full scope
validity. A descriptive identity or observation never mints authority.

Accord delegation is independent of Team/creator adjacency and accepts a live
Dormant Task. It preserves the caller's live source/ancestor, distribution,
operation, data and scope ceilings; it confers no execution-management rights.
Registration uses fixed recipient budgets (1,024 total, 256 incoming, 32 per
sending Task, 128 per sending Team) and the existing sender Heirs limit of 1,024.
Prepared and unsettled registrations count. Exhaustion returns OutOfMemory;
Busy retains continuation and hardware responsibility. Incoming saturation
preserves 768 slots for local work. Return/Revoke/exit reclaim settled charges.
See the [owner, failure and receive contract](../docs/pend-delegation.md).

| Nest right | Bit | Scope |
| --- | --- | --- |
| INSPECT | 1 | Whole or Reservation |
| RESERVE | 2 | Whole |
| RELEASE | 4 | Reservation |
| ATTACH | 8 | Whole, Internal Team adoption only |
| PUBLISH / RETRACT / PROTECT | 64 / 128 / 256 | Reservation |
| DELETE | 512 | Whole |
| INSERT / REMOVE | 1024 / 2048 | Reservation |

Bits 16 (DETACH) and 32 (TRANSFER) are invalid. `NestRights::whole()` is
INSPECT\|RESERVE\|ATTACH\|DELETE. `reservation()` is
INSPECT\|RELEASE\|PUBLISH\|RETRACT\|PROTECT\|INSERT\|REMOVE. `all()` is their union,
**not** valid authority for either single scope. Generic Accord/Narrow require
valid limits, the same scope category and the same ReservationId, and attenuate
operations, Modifiable and Distributable. They never convert Whole to Reservation,
Reservation to Whole, or one ReservationId to another.

#### Reservation and reply layouts

Nest class 13 keeps Create 0, Inspect 1 and Delete 4; slots 2/3 stay invalid.
Reserve 5 takes `(token: PendId, request: VirtAddr)` and returns `(PendId, VirtAddr)`
in `a0`/`a1`, without a leading status word. Release 6 takes `(token: PendId)` and
returns `()`. NestFail keeps Denied -1, Dead -2, Busy -3, OutOfMemory -4, and adds
NotAligned -5, AlreadyMapped -6, NoRegion -7.

ReserveRequest is six words (48 bytes): `[placement_tag, address, align, count,
level, access]`. Placement retains Lowest 0 / Highest 1 (address zero), or Exact 2,
with checked base-page/power-of-two alignment. PageCount is nonzero, byte size
must fit, and count is a multiple of CellLevel.pages(). API checks representable
levels; Kernel checks the active Sv mode. Access uses READ 2 / WRITE 4 / EXECUTE 8,
requires READ or EXECUTE and forbids WRITE without READ. Cell level does not
require a matching hardware leaf. External admission remains exact-only, one
whole level-0 or level-1 Cell within the existing IOVA bounds and aligned 2 MiB
window, READ or READ|WRITE, and unchanged private-source qualification.

Reserve atomically creates a source-free **Vacant** reservation and fresh
Reservation-scoped root Pend. It allocates no physical Page, materializes no Slot
and never publishes. The mapping root has `reservation()` operations; Modifiable,
Distributable and target write access are capped by its creating Whole. Only
Reserve establishes this special Whole-to-fresh-Reservation ancestry; it cannot
select an existing ReservationId. Its real parent-child ledger edge prevents Whole
Narrow and empty-root adoption while mapping descendants remain live.
Release requires RELEASE over the full original mapping window; a full-span
child may Release the entire relation, but child Return does not Release.
Root Return retires the full original span even after Narrow. Busy retains
metadata and forbids reuse; retry/inspection/cleanup remain available while
Closing rejects fresh admission and Accord.

Cell class 2 Create 0 takes `[license, size, align, level]` and returns SlotId
through the ordinary API pair decoder: `a0` holds SlotId, not a fixed zero status
prefix. Delete 1 takes SlotId directly. CellObject is `[tag, id]`, tag 0 Slot /
1 Pend (16 bytes). CellPosition adds `[page, generation]` (32 bytes); Slot
generation must be zero, and Kernel checks a Pend's actual family/generation.
Flat/Share source positions for Insert also require generation zero.
CellHandoff and CellAssociation each contain two positions (64 bytes).
Handoff decoding rejects either Pend endpoint.

Unit Create uses `[a0, a1, a2] = [kind, id, nest_pend]` on success.
`CreatedUnit::Task(task)` is `[0, task_id, 0]`;
`CreatedUnit::Team { team, nest }` is `[1, team_id, nest_pend]`.
IDs must be nonzero and at most isize::MAX; unknown kinds and cross-kind payloads
reject. Team's Nest is a live Whole INSPECT|RESERVE grant under the old limits,
not a Team number accepted as authority. Explicit empty-root adoption preserves
its live identity, consumes ATTACH/DELETE and binds once; automatic allocation
returns a fresh limited Whole. Grant preparation precedes Team publication and
failure changes neither the old adoption grant nor the table. Team owns the
Nest lifetime; generic grants cannot restore consumed rights. Ordinary Task
creation receives no default rights.

### Historical migration from ABI v8 / Ledger v8 to ABI v9 / Ledger v9

Rebuild all executables and regenerate Capsule format 2 with ABI byte 9. API,
Client and Server must use the same revision; old binaries fail before dispatch.

| Previous contract | ABI 9 contract | Consumer action |
| --- | --- | --- |
| Handoff `{ source, target }`, Push/Pull aliases | `{ local, remote }`; Push moves local to remote, Pull reverses that direction | Supply two owning Slots and independently authorize both |
| Push/Pull performs association | Insert (slot 10) takes 96-byte CellAssociation; Remove (11) takes 48-byte CellPosition | Use Insert for Slot/Memo to Memo, Remove after hardware withdrawal |
| Memo object `{ target, mapping }` | 32-byte `[1, PendId, 0, 0]`; positions still 48 bytes | Delegate Memo Pend, never reconstruct control from a ReservationId |
| Create Memo returns ReservationId | Returns root Memo PendId in the same reply register | Store `CreatedCell::Memo { token, address }`; Pend Inspect reports stable MemoId |
| MappingControl Caller/SpaceMembers | Removed; creation word 11 reserved zero | Explicitly Accord the required Memo rights to other Tasks |
| Flat operations 0–3 | Block (4), Allow (5), independent BLOCK bit 32 | Retain the exact blocking Pend until completion/reopening |
| Pend target/permission kinds | Memo is kind 7; seven rights bits 1 through 64 | Decode MemoId and MemoRights; no access from a descriptive identity |
| Ledger 8 | Ledger 9, same 40-byte entries, Memory Share/Sign pair | Discover `Name::Memory`; its Share is read-only global telemetry |

All error domains retain their codes. Denied rejects missing/live-authority
mismatches; NotAligned rejects malformed windows/Cell heads; Busy requires a
fresh Inspect or retry of retained withdrawal. Failed transfers preserve both
Slots. Partial hardware withdrawal and failed publication keep their exact
responsibilities; returning Busy is never a physical reuse certificate.

### Historical migration from ABI v7 to ABI v8

Rebuild executables and regenerate Capsule format 2 with ABI byte 8.
Call slots, descriptor layouts and the resource Ledger version stay the same.
Share grants now permit overlapping CPU mappings. Callers that need a stable
copy can use Clone. Callers that need to freeze the original Page must
withdraw other writable publications before Cell Retract can succeed.

### Historical migration from ABI v6 to ABI v7

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

Ledger v10 keeps the v7 40-byte entry shape: name `[0,16)`, resource kind at 16,
optional View tag at 17 (0 absent, 1 Clone, 2 Share), zero reserved bytes
`[18,24)`, primary Pend at `[24,32)` and View Pend at `[32,40)`. Handles are
little-endian u64 values. `Name::Interrupt { controller, source }` requires a
Share View and Sign pair; `Entry::dispatch()` returns the explicit View kind
and both handles. `Name::Memory` also requires a Share/Sign pair. Its companion
`Name::Page(Page::Memory)` names the same Share. Other entry kinds, including
Nest, have no paired View.

`Name::Space` is name class 7/item 0 with all remaining name bytes zero, and
ResourceKind::Nest. It supplies the initial current CPU Nest's live Whole
INSPECT|RESERVE grant, with no ATTACH/DELETE. `Name::Nest { controller, requester }`
(class 5) instead names a trusted External Nest root. Neither name, topology nor
Team membership is accepted as authority; callers must retain the actual Pend.
Malformed, incomplete and old-version ledgers fail validation. Sign operations
retain their slots and payloads under the current ABI prefix.

A source admits one non-distributable Listen/Consume branch only when that Task
holds the matching full writable Share View with Open permission. Listen-only
observers may be derived independently. Initial device inspection and real IRQs
both use ordinary retained sequences and Consume. The source interprets successful
consumption asynchronously; revocation/cancellation does not acknowledge hardware.

### Cell contracts

CellObject is 16 bytes, CellPosition 32, CellHandoff and CellAssociation 64,
and CellInfo 64. Caller-memory descriptors are copied once before decoding.
Keep inputs readable and outputs writable through return. No pointer is retained.
Cell Create is allocation-only and returns a Task-local SlotId. Nest Reserve is
source-free and returns a mapping root Pend plus virtual address. ReservationId and
PageId are descriptions, not authority. CellObject::Pend accepts only a live
Pend in the calling Task's ledger; Kernel checks its actual family and scope.
Pend positions are relative to that grant's current effective window; Slot
positions are relative to their owning handle. Windows cover complete physical
Cells. Team membership and management alone confer no control.

| Call | Required authority and state | Effect |
| --- | --- | --- |
| Cell Create | None FLAT, valid Allocation | Reserve lazy owning capacity; moved-out Slots never regenerate Pages |
| Split / Merge | Private exclusive complete contiguous Slots; no aliases or hardware obligations | Repartition unique allocation receipts; replace Slot only on success |
| Nest Reserve | Whole Nest RESERVE, valid ReserveRequest | Atomically reserve Vacant positions and fresh mapping authority; no Page or implicit publication |
| Cell Delete | Owning Task-local Slot handle | Release the handle; existing mapping/hardware obligations survive |
| Nest Release | Reservation RELEASE over its full original window | Retire associations and reservation; Busy retains cleanup |
| Push / Pull | Independent ownership at both Slot endpoints; Flat TRANSFER where governed by Flat | Atomically move the unique holding; no association or hardware change |
| Insert | Source Slot association authority, Flat/Share ASSOCIATE or Reservation INSERT; destination Reservation INSERT | Associate the same Page after source compatibility and generation checks; never create another owning Slot |
| Remove | Reservation REMOVE, unpublished and settled position | Clear association/freeze, advance generation; leave owning Slot untouched |
| Publish | Reservation PUBLISH, live original source authority, Ready generation | Recheck the exact held Page and mapping permissions; WRITE also requires Modifiable Yes |
| Retract | Reservation RETRACT, Published generation (External also Publishing) | Withdraw this hardware use, retaining obligations until completion |
| Protect | Reservation PROTECT, CPU position and original access ceiling | Change access/generation; WRITE requires Modifiable Yes; External rejects |
| Inspect | Source ownership or Reservation INSPECT | Observe without obtaining mutation rights |

Push/Pull decode only two owning Slot endpoints. Insert accepts a Slot or
qualified source Pend and requires a Reservation-scoped destination. A mapping
observation cannot extract an owning Slot; resolve and independently authorize
the actual owning Slot for a transfer. Reservation-to-mapping Insert copies an
association, retaining the **original physical source branch, actor and Page**.
Its sticky mutability ceiling is the minimum of the original association/source
mapping ceiling and the supplying grant's Modifiable. There is no new owning
Slot or second permission family. Later Return/Narrow of the supplying mapping
grant does not retroactively revoke this independent destination association;
loss of the actual original-source branch or actor still withdraws its uses.

Physical ownership movement preserves the old association, Page and generation,
but a new Publish fails when the selected source no longer holds that exact Page.
Insert from the new source is required. Returning/revoking a child mapping grant
withdraws publications made through that branch, not a sibling's publication.
Inspector-only branches own no publication. Child Return never releases the whole
mapping. Root Return retires its full **original** reservation even after Narrow;
RELEASE can also be delegated to a child retaining the complete original window,
whose Release destroys the entire relation rather than only that branch.

Reservation identity survives reassociation. Stable states are Vacant, Ready(Page)
and Published(Page); Publishing/Retracting are retained flights, Failed retains
an unresolved hardware error. Insertion/removal, publication, retraction and
protection advance a non-wrapping generation. Inspect refreshes after Busy;
replaying an old generation never starts a later cycle. Slot positions use
generation zero and act on the current holding, not a historical snapshot.
A source-free reservation never materializes or publishes backing through a CPU
page fault. A fault on a Vacant position waits for explicit Insert/Publish while
the reservation remains live; Release makes that wait invalid. The missing PTE
is not permission to remove or reuse the reservation.

Retract returns Busy before changing the mapping if another publication still
retains write access to the Page. Successful Retract retains a persistent freeze
in the mapping. `frozen` proves the admission gate is closed, no writable hardware
responsibility remains, and the Page is not quarantined. A sequence check alone
proves none of this. An independent CPU mapping's freeze blocks a writable CPU
publication. An authorized external handoff can use the sole freeze after CPU
withdrawal, subject to exclusive Page access. Explicit read-only publication may
preserve a frozen snapshot. Publish, association replacement or Release ends that
mapping's freeze. Failed publication may advance the generation and leave Ready
with its freeze, so retry from Inspect. Historical observation references do not
prolong deleted freezes after fencing.

External Retract may return Busy after starting an asynchronous withdrawal.
Inspect distinguishes a rejected request from a retained Retracting flight with
an advanced generation. Wait for that exact generation's ReservationRetired event
or inspect settled Ready state before treating this mapping's withdrawal as
complete; Busy alone proves neither acceptance nor completion.

Retract completion settles only this mapping. `external` and other retained
CPU/device obligations may still forbid moving or reclaiming the Page. Physical
reuse requires all such obligations to settle, independently of business results,
Sign notifications, timeouts, handle deletion or Task exit. Ready is not a claim
that memory has been freed. Task death and creating-branch revocation retain
hardware records through completion. Their orphaned mappings, including unused
reservations, are then removed; later lookup of the old ReservationId fails.

External mappings use one complete level-0 or level-1 Cell, aligned to its size,
a Reservation-scoped Nest Pend and READ or READ|WRITE. Reserve is Exact only;
private-source admission at Insert/Publish is not relaxed. Overlaps reject.
One binding supports up to 512 base-page Cells or one level-1 Cell within the
aligned 2 MiB window selected by its first mapping. Positions outside that window
are rejected. Pend handover does not reset this hardware window. A settled mapping
can be reassociated and republished; retracting one leaf leaves other leaves
accessible. Page substitution requires explicit Cell Insert and a fresh generation
from Inspect. Each range retirement requires matching invalidation/fence completion
and trusted platform ordering. Binding retirement is a separate, whole-context
boundary, not performed on every Pend handover. Unsupported aliases, shared
controllers, ATS/PRI and noncoherent access are rejected.

The former Cell Acquire/Release receipt interface, Attach/Detach, Begin/End,
Traverse, Position, Attachment and general Retain authority are removed. Their
replacements are Cell objects, qualified transitions and kernel-owned hardware
retirement, not a public change receipt or duplicate reference lifecycle.
Nest Release retires a mapping reservation; it does not restore that receipt
interface.

Split/Merge operate on owning Slot collections, not PTE leaves. Flat/Share roots
are not eligible. Every source Slot must be materialized; merge requires physical
adjacency. New metadata and the replacement handle are prepared before receipt
commit, so allocation failure preserves the original collection.

### Flat Block / Allow

Both calls require Flat BLOCK independently of TRANSFER, ASSOCIATE, Modifiable
and Distributable. The effective Flat window is the block's extent. Once
registered, the block persists across Busy and failed hardware withdrawal.
Block excludes new associations and hardware publication, withdraws existing
CPU and External uses, and succeeds only after their exact obligations drain.
It preserves mapping associations. It never treats a device notification as fencing.
An untracked alias still retaining the Page causes Busy until that alias retracts.

The exact holder may continue authorized Slot transfers and associations using
its Flat branch. Incoming Pages enroll in the same Page gate before commit;
unsettled old hardware use rejects the move without changing either Slot.
Other branches cannot use this exemption. Page gates also cover aliases after
ownership movement. Allow removes only the calling branch's block, and does not
republish mappings or restore revoked rights. Allow before Block completion
cancels that block; it does not assert safe reuse. Narrow rejects a live block.
Return/Revoke/Task death retains the block while its old hardware users retire,
then drops that branch's gate. Other blocks remain effective.

### Global memory observations

Ledger 10 `Name::Memory` (name class 6, item 0) provides `(Share, Sign)`;
`Name::Page(Page::Memory)` (class 2, item 3) names that Share. Initial access goes
only to the bootstrap Task, which can explicitly delegate attenuated grants.
Share is Modifiable No with OPEN/SHUT/ASSOCIATE; Sign permits LISTEN/CONSUME.
No receiver can trigger events, delete these kernel-owned endpoints or elevate
access. Return/Revoke withdraws a recipient's mapping through ordinary Share
retirement. A retained pointer must not be dereferenced after revocation.

`model::memory::SnapshotPage` is one aligned 4096-byte page. Word 0 is an atomic
publication sequence (odd while publishing); words 1–13 are protocol version 1,
epoch, start/end nanoseconds, capacity, available, resident, CPU retained bytes,
I/O retained bytes, retiring bytes, abandoned bytes, accessed and dirty.
All words are little-endian u64; spare words remain zero. `read()` performs one
bounded atomic attempt and returns None for an unpublished, concurrent or
unsupported sample. Retry with a caller-chosen deadline, retaining the Share.

| Field | Coverage |
| --- | --- |
| capacity | Bytes admitted to the managed frame allocator; excludes reserved firmware/image/bootstrap ranges |
| available / resident | Free managed bytes / allocated managed bytes, including heaps, page tables and Task stacks; sampled together, sum equals capacity |
| cpu_retained / io_retained | Bytes in outstanding Page hardware obligations, including preparation and retirement; aliases count separately |
| retiring | Obligation bytes with withdrawal started but not settled |
| abandoned | Permanently retained obligation bytes after lost completion responsibility; also remain in retained totals |
| accessed / dirty | `UNKNOWN = u64::MAX`; per-page A/D is unsupported in this preview |

Multi-Level allocations count actual allocated bytes, not allocation events.
Retention counters belong to Page's responsibility transitions; the observer
holds no resource registry. Allocator and retention samples are taken within
`[start_ns, end_ns]`, not a simultaneous system-wide transaction. Overflowed
retention metrics latch UNKNOWN. No sample exposes physical addresses or raw
PTEs, conveys a Pend, or proves any Page safe to reclaim.

A resident ordinary Task in Kernel Team samples these fixed-size owners every
100 ms using Room SleepUntil and existing scheduling. Its code, stack and output
page require no external pager. Sign coalesces updates until Consume; a sequence
notifies availability, while Snapshot.epoch identifies the sample. Consumers
should consume the notification then re-read/recheck their epoch before waiting.
Stopping a consumer or the producer cannot block allocation or hardware
retirement. Root shutdown terminates the producer through ordinary Task Exit.

### Runtime organizations

Flat Create takes `(license, size, align, level)` and returns a Flat-target root Pend;
it reserves lazy Slots using the frame allocator. Flat Inspect writes 32 bytes
(FlatId, effective page offset, count, level). Flat Slice returns a Task-local SlotId for a relative window;
that Slot retains the exact live Pend, not a copied permission snapshot. Every
use rechecks the current branch/window. It cannot bypass Narrow, suspension or
Revoke. Flat Delete seals the source and withdraws associations; Busy keeps cleanup
responsibility. Cell Delete of a Slot releases only that handle.

Share Create on a Flat Slice requires None SHARE, Flat SHARE, Distributable Yes
and a valid source window. It creates a Share child in the same Pend tree. Flat
ASSOCIATE permits Share OPEN/SHUT/ASSOCIATE; Flat DELETE alone permits the child's
DELETE right. Generic Accord never changes target kind. Flat TRANSFER is checked
separately at both ends of Cell Push/Pull into a Slot; it cannot be inferred from
read/write access. A private allocation retains its original source ownership
when adopted by its own Share root; delegated Share branches do not inherit
that ownership. A mapping never supplies owning-transfer authority. Deriving a
CPU Share Pend preserves the parent's existing mappings. Authorized CPU mappings of the same RAM Page may coexist, with write
permission checked separately for every branch. The caller coordinates writes
and observations through its shared-memory protocol. Device register Views
use exclusive writable custody. Delegation of such a View withdraws the
writable parent's mappings until the child access has settled.

Nest Create takes a None NEST license and returns an Internal root. External
Nests are created only by trusted boot admission, which authenticates the complete
controller/requester isolation boundary before delivering an ordinary Nest Pend.
The Ledger name describes that topology; it is not accepted as a runtime credential.
Nest Inspect writes 56 bytes with stable NestId, kind, phase and retained fault
information. Public Port identities, rights, calls and a second authorization
lineage do not exist.

Keep the External Nest Whole root with its manager. Accord delegates Whole
INSPECT/RESERVE, Modifiable and Distributable to workers, which can reserve fresh
mapping roots under those limits. Reservation operation grants remain scoped to the
same ReservationId/window. A worker cannot widen its rights or acquire another
source by supplying a raw number. Death, Return or Revoke invalidates its branch
and retires its descendant mappings, not an unrelated Whole root or branch.
The exact creating branch remains retained until its positions are idle and
removed; live publication additionally retains its actual publishing permission
and original source borrow. Other branches keep their live leaves. Only one Task
at a time may reserve positions in an External Nest. A competing reservation
returns Busy until the former Task's positions have all been removed. Reusing an
old address requires hardware settlement; a replacement has a new ReservationId.

Root loss, Delete by the External Nest root with DELETE permission, or an unsafe backend failure
closes the entire address space. Delete starts irreversible Closing and returns
Busy while binding retirement or cleanup remains; retry completes deletion.
Closed cannot reopen. A private Unbound state exists only during trusted boot
preparation, never as a user opportunity to attach arbitrary hardware. The
controller stays assigned until the backend's exact complete binding and platform
proof; failure retains Pages, table pages and queues. No temporary driver Task
is the sole owner responsible for completing this cleanup.

Sign Attach includes `generation`. ReservationPublished (tag 11) and ReservationRetired
(tag 9) retain `{ token, reservation }`, but require live **Reservation-scoped Nest INSPECT**
for that same ReservationId and the full original mapping window, with the expected
generation. The raw ReservationId is an observation consistency check; Whole INSPECT
plus a raw ID never substitutes.
Existing External-only observation support is unchanged. Completion before
registration is retained; the immediate next generation may be armed before
publication. Older generations report Gone rather than matching replacements.
NestClosed (13) and NestFault (12) require Whole INSPECT, generation zero and the
existing External support. Hardware FQ records have no software mapping generation:
the oldest unattributed fault stays at Nest scope, subsequent faults increment a
saturating sequence, and no unbounded queue or guessed mapping attribution is
exposed. Listen/Consume and Pend handover do not clear Nest faults. MappingFault
(tag 10) stays invalid. Whole-Nest close and branch retirement remain distinct.

| Operation | Required permission | Additional boundary |
| --- | --- | --- |
| Flat Create / Cell allocation | None FLAT | Checked allocation geometry |
| Flat Inspect / Slice / Delete | Flat INSPECT / ASSOCIATE / DELETE | Current window; Delete retains failed withdrawal |
| Cell Push/Pull to a Flat Slot | Flat TRANSFER on each Flat endpoint | ASSOCIATE handle, Modifiable Yes, movable Page |
| Share Create from Flat | None SHARE + Flat SHARE | Distributable, narrowing and source qualification |
| Nest Create | None NEST | Internal Whole root; no hardware source parameter |
| Team Create with Internal Nest | Build + Whole ATTACH | Empty exclusive root, no descendants/borrows; preserve live identity and return limited Whole |
| Nest Inspect / Delete | Whole INSPECT / DELETE | External Delete requires exact boot root; Busy retains cleanup |
| Nest Reserve | Whole RESERVE | Fresh source-free Reservation root, never arbitrary existing ID |
| Nest Release | Reservation RELEASE | Complete original window; Busy is not reuse permission |
| Cell Insert / Remove / Publish / Retract | Reservation INSERT / REMOVE / PUBLISH / RETRACT | Qualified source, exact generation and original source access |
| Cell mapping Inspect | Reservation INSPECT | Same mapping identity and effective window; observes the current generation |
| Sign mapping conditions | Reservation INSPECT | External only; same ReservationId, full original window and expected generation |
| External Protect | Unsupported | Reject; CPU Protect requires Reservation PROTECT |

External Protect stays unsupported; External Delete requires its exact boot
Whole root. None grants no external source creation, raw hardware access or
arbitrary syscall permission. Team adoption consumes Whole ATTACH/DELETE and
caps CPU Whole authority to INSPECT|RESERVE under its previous limits. The Team
owns that Nest's lifetime; membership/manager authority and generic Accord cannot
restore the consumed bits or replace a live scoped Pend.

### Authority and control contracts

Pend Accord snapshots 104 bytes; Narrow snapshots 80 bytes. Both carry typed
operations, independent Modifiable/Distributable and a checked PendScope.
Derivation requires Distributable. Neither operation adds rights, changes target
family or scope category, converts Whole/Reservation, or changes a ReservationId.
A requested Pages range is relative to the immediate parent's/current effective
window; the server resolves bounds at commit. Published 128-byte PendInfo uses
absolute Pages and reports target identity, full scope and effective limits.
FlatId, NestId and ReservationId are stable descriptions, never authority.

Narrow retains identity and rejects live descendants with Busy, including mapping
descendants of a creating Whole grant. It withdraws publications made through the
narrowed branch before committing reduced limits; a sibling's publication is not
that branch's use. Active data borrows and publication routes must settle where
required. Retirement-only holds keep cleanup responsible without preserving
permission or permanently preventing Narrow.

None, Sign, Flat, Nest and Views share one live Pend ledger and derivation tree.
None is object-free call permission: Object scope, Modifiable No and only the
listed CreationRights. Clone and Sign also use Object; Flat/Share use Pages;
Nest uses Whole or Reservation with the matching rights mask. Sign requires
Modifiable No; LISTEN/TRIGGER/CONSUME/ROUTE/DELETE are control rights, not mutable
memory. The Kernel mints the bootstrap None root in `Name::Call(Call::Create)`.
Ordinary Task creation supplies no default rights; managers must explicitly
Accord permissions before a new Task invokes resource creation. Changing a raw
number, target or operation identity cannot bypass live grant validation.

| Live View operation | Required operation permission | Additional condition |
| --- | --- | --- |
| Clone Push | Clone PUSH | Modifiable Yes; queue capacity and payload bounds |
| Clone Pull | Clone PULL | A live queued message; payload becomes an independent read-only Share |
| Clone Peek / queue condition | Clone PEEK | Live queue |
| Clone/Share Delete | Matching DELETE | Live branch; closing Share can retry pending withdrawal |
| Share Open | Share OPEN | Effective range, Modifiable and Page admission |
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

Unit Create snapshots a 48-byte Team or Task descriptor. Tasks start dormant
and receive no default rights. The three-register result is CreatedUnit, not
UnitTarget. Team creation requires Build authority and returns `{ team, nest }`;
`nest` is a live CPU Whole INSPECT|RESERVE Pend capped by its old limits.
Its optional `nest: PendId` (word 3, zero allocates a new Nest) adopts an empty
Internal Nest. The caller must hold its exclusive root Whole ATTACH grant with
Modifiable Yes, no descendants and no borrows/hardware holds. Success preserves
that live root identity, removes ATTACH/DELETE and binds it once to the Team,
which owns the Nest's lifetime. Automatic allocation returns a fresh limited
Whole. The returned grant is prepared before publishing the Team; failure leaves
both an old adoption grant and the table unchanged. External Nest, delegated
roots, kind mismatch and repeated adoption reject. TeamId, membership or manager
qualification alone cannot authorize CPU mapping calls. This never attaches an
External Nest to CPU execution.
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
| Share Create | None SHARE license and authorized Slot/source window | New View root or narrower branch; source qualification and range checks apply |
| Clone/Share Delete | Matching target DELETE | Closes object; unfinished borrows and hardware obligations remain |
| Clone Push | Clone PUSH and Modifiable Yes; queue limits | Enqueues one copied message or fails; no implicit retry |
| Clone Pull / Peek | Clone PULL / PEEK | Pull delivers message authority; Peek observes length/sender/count without consuming |
| Share Open / Shut | Share OPEN / SHUT; range, Modifiable and Page admission | Opens/closes this caller's access using the common Nest publication machinery; Shut neither returns Pend nor deletes View |
| Pend Accord | Delegable parent and non-expanding relative window/access | New local/remote branch; failed preparation does not publish partial authority |
| Pend Revoke / Return | Direct grantor / holder | Retires the branch and its descendants; Busy retains withdrawal obligations |
| Pend Narrow | Holder; no live descendants or conflicting data borrow | Withdraw own publications before reducing limits; Busy retains routes; retirement-only retention is not live permission |
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
| Room Inspect (`Ordinary`) | Ordinary callers | Slot 6, query tag 0 in a0. Returns the number of online Harts without a real-time reservation in a0. The snapshot may change between calls. Older ABI v7 providers report an unknown call. |
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
migration describes ABI v4; current binaries must use v10 and the layouts above.

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
| One-shot single-page external route | Repeatable mapping positions under one persistent Nest | Retract a leaf; Release a mapping; Delete only the entire owning Nest |
| Sign Attach without generation; MappingFault | Generation-qualified ReservationPublished/Retired and NestFault | Pass generation explicitly, zero for non-mapping sources; retain scoped observation authority |
| Team descriptor word 3 reserved zero | Optional owned Internal Nest root | None allocates; adoption preserves live identity and returns limited Whole |
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

The [M6 integration record](memory-m6.md) describes the opt-in ABI 11 image,
real Team/CPU/IOMMU/Virtqueue checks, performance scope and unverified hardware.
