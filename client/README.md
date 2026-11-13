# Kernel Client

The [Reservation migration](../api/memory-reservation.md) changes address-reservation
Rust names across API and Client without aliases. Existing source consumers must
update and rebuild; running v10 wire labels and layouts remain unchanged. This
does not activate the staged ABI 11 calls.

`kernel-client` is a `no_std` library for executing Kernel calls. It depends on
`kernel-api` and the declared error type, and needs no allocator or runtime state.

| Module | Responsibility |
| --- | --- |
| `call::{cell,unit,room,...}` | Named typed calls, including terminal `unit::exit` |
| `invoke` | Execute a returning or terminal API operation |
| `transport` | RV64 `ebreak` register boundary |

Import model and ABI types directly from `kernel_api`. Client owns no type
prelude, heap, mapping ownership wrappers, closure task model, synchronization,
TLS registry, startup adapter or exit policy. Language runtime/std work is
separate from Kernel. The former `__private` adaptation interface is removed.

`invoke_with(call, transport)` packs inputs once, calls the supplied transport
once and decodes the result once. It is the host-test entry point.
`unsafe invoke(call)` selects the native trap; its caller must satisfy execution
context, pointer validity and resource lifetime requirements. `invoke_terminal`
enters a non-returning fault path if a terminal trap unexpectedly returns; it
uses neither TLS nor a panic handler. Named calls do not hide retry, ownership,
publication or cleanup policy.

Each domain declares its named signatures under `src/call`. The shared macro
executes the typed requests defined in API. `unit::exit` takes result, reason,
readable diagnostic note and explicit ExitScope; note length is bounded by
`abi::call::unit::NOTE_MAX`. Task scope exits only the caller. Team scope requires
a non-realtime member of a Team configured with `member_exit`. A return violates
the terminal contract. `deadline_after(Duration)` converts once using Room Clock;
retain the resulting Wait across retries rather than extending the deadline.

## Migration from ABI 9 / Ledger 9 to ABI 10 / Ledger 10

Rebuild API, Client, Server and executables together. Capsule format remains 2,
but byte 11 must declare ABI **10**, and Ledger version is **10**. Old and future
ABIs reject before dispatch. No compatibility aliases retain MemoId, MemoRights,
MappingTarget, CellCreate, CellSource, MemoDestination or CreatedCell. Pend target
and operation kinds 5 (Port) and 7 (Memo) are invalid, not repurposed.

| Previous call | ABI 10 call | Result |
| --- | --- | --- |
| `cell::create(license, request)` | `cell::create(license, allocation: Allocation)` | SlotId, ordinary two-register convention |
| `cell::delete(object_address)` | `cell::delete(slot: SlotId)` | `()` |
| Cell Create mapping variant | `nest::reserve(token, placement, count: PageCount, level: CellLevel, access: u64)` | `(PendId, VirtAddr)` in two success registers |
| Reservation deletion | `nest::release(token)` | `()`; Busy retains cleanup |
| `unit::create(request) -> UnitTarget` | `unit::create(request) -> CreatedUnit` | Three success registers; never discard Team's Nest grant |

Cell class 2 keeps slots 0/1 for allocation/deletion. Create packs
`[license, size, align, level]`; no caller-memory creation descriptor remains.
The ordinary API pair decoder takes SlotId from `a0`, not from a payload after a
fixed zero status. Reserve returns its PendId in `a0` and virtual address in `a1`.
Allocation uses `Allocation::new` or `Allocation::at_level`. Nest class 13 keeps
Create 0, Inspect 1, Delete 4; Reserve is 5 and Release 6. Slots 2/3 stay invalid.
NestFail preserves Denied -1, Dead -2, Busy -3, OutOfMemory -4 and adds NotAligned
-5, AlreadyMapped -6, NoRegion -7.

`nest::reserve` constructs one local 48-byte ReserveRequest snapshot and invokes
`operations::Reserve { token, request }` once. Its six little-endian u64 words are
`[placement_tag, address, align, count, level, access]`. Placement is Lowest 0 or
Highest 1 with address zero, or Exact 2, using existing alignment policy.
Count is nonzero, its bytes must fit, and it must be a multiple of level.pages().
API checks representability; Kernel checks the active Sv mode. Access is READ 2 /
WRITE 4 / EXECUTE 8, nonempty in READ or EXECUTE, with WRITE requiring READ.
Cell level is not necessarily a hardware leaf. External admission stays Exact
only, one whole level-0/1 Cell within the existing IOVA bounds and aligned
2 MiB window, READ or READ|WRITE and unchanged private-source qualification.

Reserve requires Whole RESERVE and atomically returns a source-free **Vacant**
reservation plus a fresh Reservation-scoped root. It creates no physical Page or
owning Slot and does not publish. The root has `NestRights::reservation()` operations;
Modifiable, Distributable and target write access are capped by the creating
Whole. Only Reserve creates this Whole-to-fresh-Reservation ancestry; it cannot pick
an existing ReservationId. The real parent-child ledger edge makes live mapping
descendants block Whole Narrow and empty-root Team adoption.

### Scoped delegation and observations

Limits now contains `operations`, `modifiable`, `distributable`, `scope`, not a
separate range. Use `PendScope::Object` for None/Clone/Sign, `Pages(PendRange)` for
Flat/Share, and `Nest(NestScope)` for Nest operations. None/Sign require Modifiable
No. Whole permits only `NestRights::whole()` (INSPECT 1, RESERVE 2, ATTACH 8,
DELETE 512). Reservation permits only `reservation()` (INSPECT 1, RELEASE 4, PUBLISH 64,
RETRACT 128, PROTECT 256, INSERT 1024, REMOVE 2048). Bits 16/32 remain invalid;
`all()` is the union, not valid for either single scope. Generic Accord/Narrow
cannot switch scope category, convert Whole/Reservation or change ReservationId. Rights,
mutability and distribution only attenuate; read-only and non-distributable
limits cannot be restored by deriving another grant.

All words below are little-endian u64:

| Descriptor | Layout | Size |
| --- | --- | --- |
| PendScope | `[tag, id, range_tag, offset, count]` | 5 words / 40 bytes |
| Limits | `[operation_kind, operation_bits, modifiable, distributable, Scope...]` | 9 words / 72 bytes |
| AccordRequest | `[src, recipient_tag, recipient_id, mark, Limits...]` | 13 words / 104 bytes |
| NarrowRequest | `[token, Limits...]` | 10 words / 80 bytes |
| PendInfo | `[token, target_kind, target_id, owner, lender, mark, status, Limits...]` | 16 words / 128 bytes |

Scope tags are Object 0 (all payload zero), Pages 1 (id zero), Nest Whole 2
(all payload zero), Nest Reservation 3 (nonzero valid ReservationId). PendRange is
`[0,0,0]` for Inherit or `[1,offset,count]` for nonempty, nonoverflowing Pages.
Unknown/reserved/noncanonical payload rejects. Requests accept Inherit and
relative Pages. The server resolves bounds at commit; pure Limits checks are
not a substitute. Published PendInfo uses resolved absolute Pages and validates
the full scope as well as target family. ReservationId and NestId are descriptive,
not capabilities; Team membership/management and raw IDs never replace a Pend.

`pend::accord(request)` takes the address of `AccordRequest::to_bytes()`; retain
that 104-byte snapshot through return. Use AccordTarget::Caller for local
derivation or Task for delegation. `pend::narrow(request)` similarly reads an
80-byte NarrowRequest. Return relinquishes the holder's branch through Kernel
retirement. Busy retains cleanup; no call implies safe physical reuse. The old
Pend derive/same/release and ReleaseMode interfaces do not exist.

Task delegation accepts unrelated Teams, including a live Dormant recipient.
The caller must hold the live, distributable source with every requested right
and range; delegation grants no Task management authority. Registration has
fixed budgets: 1,024 total slots, at most 256 incoming slots, 32 per sending Task
and 128 per sending Team at one recipient. Source Heirs retain their existing
1,024-edge limit. Prepared and unsettled grants count; preparation failure rolls
back, and Return/Revoke/exit release settled registrations. Exhaustion returns
`PendFail::OutOfMemory`; contention returns Busy. Multiple Teams can fill the
incoming partition, while 768 slots remain available for local work. No receiver
call or prior message is required before an initial grant. See the
[registration and effect contract](../docs/pend-delegation.md).

### Next-memory Flat/Nest interface

`kernel_client::memory::{flat,nest}` is the next ABI interface. Construct the
corresponding `Call` from a validated Action and explicit RequestId, then use
`flat::invoke` or `nest::invoke`. `memory::invoke_with` provides an explicit host
transport. Calls snapshot fixed-size buffers, perform one invocation and check
result/observation correlation; they never retry automatically or fall back to
Cell. A Delivery error does not imply rollback. InspectResult recovers the
original request, including its original mutation opcode.

This interface requires the coordinated v11 provider. The running v10 Kernel
rejects it; activation remains gated by M3–M6. See the [M2 acceptance record](../api/memory-m2.md).

### ABI v10 Cell compatibility and mapping lifetime

| Descriptor | Layout | Size |
| --- | --- | --- |
| CellObject | `[tag, id]`, tag 0 Slot / 1 Pend | 16 bytes |
| CellPosition | `[object_tag, id, page, generation]` | 32 bytes |
| CellHandoff | `[local_position..., remote_position...]` | 64 bytes |
| CellAssociation | `[source_position..., target_position...]` | 64 bytes |
| CellInfo | Existing generation/state/geometry observation | 64 bytes |

Slot generation must be zero. Kernel validates a Pend's actual family and
generation; Flat/Share source positions for Insert also use generation zero.
Positions are relative to the Slot handle or the Pend's effective window.
Push moves local to remote; Pull moves remote to local, and decoding requires
two owning Slots. Insert accepts a Slot or Flat/Share ASSOCIATE or Nest Reservation
INSERT source; the destination requires Reservation INSERT. Insert/Remove change
associations, never transfer ownership or extract a Slot from a live Pend.
Other mapping transitions require their corresponding scoped Nest rights.
Inspect ignores the supplied mapping generation and observes the current one;
native calls never retry automatically.

A mapping-level Insert preserves the original physical source branch, actor and
Page, with a sticky ceiling equal to the minimum of the original association/source
mapping ceiling and supplying grant Modifiable. It creates neither a second owner
nor an extra permission family. Later supplying mapping Return/Narrow does not
revoke the independent destination association; loss of the actual original-source
branch or actor does withdraw its uses. After physical ownership moves, an old
association/Page/generation remains observable, but new Publish fails if that source
no longer holds the same exact Page. Insert from the new source is required.

Release requires RELEASE plus the complete original window. A full-span child
may Release the entire relation. Child Return/Revoke instead withdraws that
branch's and its descendants' publications, not a sibling's; an inspector-only
branch owns no publication. Root Return retires the full original reservation
even when narrowed. Busy forbids reuse and retains retry metadata. Closing rejects
new admission/Accord but keeps inspection, retry and cleanup. Sign notifications,
Task death and device acknowledgements are not exact CPU/IOMMU retirement receipts.
External Protect stays unsupported; External Delete needs the exact
boot Whole root. Keep that root with its manager and delegate only needed rights.
Returning/revoking a worker Whole branch retires its descendant mappings, not an
unrelated Nest root. Competing External reservers wait until the old Task's
positions have all retired and been removed.

### Team creation and boot discovery

Public `UnitCreate::Team` creates only User Teams and requires Build authority.
There is no `kind` field or public `ProgramKind`; descriptor word 1 is reserved
zero. Retired Supervisor and unknown values reject even for S-mode callers.
See [construction and version migration](../api/memory-acceptance.md).

Unit Create success uses `[a0, a1, a2] = [kind, id, nest_pend]`.
`CreatedUnit::Task(task)` decodes `[0, task_id, 0]`.
`CreatedUnit::Team { team, nest }` decodes `[1, team_id, nest_pend]`.
IDs must be nonzero and at most isize::MAX; unknown/cross-kind replies reject.
The Team result includes live Whole INSPECT|RESERVE under its old limits. Optional
`UnitCreate::Team::nest` adopts an empty exclusive Internal root with no children
or borrows: success preserves its live identity, consumes ATTACH/DELETE and binds
it once. Automatic allocation returns a fresh limited Whole. Kernel prepares the
grant before publishing the Team; failure leaves the table and any old adoption
grant unchanged. Team owns Nest lifetime; generic grants cannot restore consumed
rights. Ordinary Task creation supplies no default grants.

`nest::create(license)` creates Internal only. External Nests arrive through
trusted boot as `Name::Nest { controller, requester }` (name class 5).
`Name::Space` (class 7, item 0, all other bytes zero) is the initial current CPU
Whole INSPECT|RESERVE grant, ResourceKind::Nest. Neither name nor membership is
authority. There is no runtime external Attach/Detach, caller-supplied requester
or ambient Team destination. `nest::inspect` writes 56-byte NestInfo.

Sign Attach keeps `{ token, reservation }` for ReservationPublished/Retired and requires
Reservation-scoped Nest INSPECT for that same ReservationId and full original mapping
window, with the expected generation; a Whole grant plus a raw ReservationId cannot
substitute. NestFault/Closed use Whole INSPECT and generation zero.
Existing External-only support is unchanged;
MappingFault stays retired. Fault observations survive Pend handover.

Resource creation requires explicit None-target licenses. Boot Ledger v10 keeps
`Name::Call(Call::Create)`; managers must delegate creation rights to new Tasks.
Memory keeps its existing Share/Sign dispatch pair and 40-byte Ledger entries.
Discover `Name::Memory` through `Entry::dispatch()`, Open its Share and retain it
while reading `model::memory::SnapshotPage`. `read()` is one bounded atomic
attempt; None asks for a later retry. Consume then recheck Snapshot.epoch to avoid
coalescing races. A/D is UNKNOWN; statistics authorize neither transfer nor reuse.

Hardware dispatch is still an explicit `(View, Sign)` pair. Delegate a device's
full writable Share before its non-distributable Listen/Consume Sign, rolling
back the View on failure before resuming a recipient. Initialize the device,
Listen and Consume the initial notification; thereafter clear the device cause
before consuming its sequence. Return/Revoke either Pend closes delivery.
Consumption is not controller completion, and raw device/configuration authority
must be withdrawn separately before reassignment. Pend distribution proves no
DMA quiescence. Retain the exact Flat BLOCK branch across Busy; Allow reopens
only its own block and never republishes. `cell::split/merge` replace an exclusive
complete contiguous owning collection only on success; hardware leaves are separate.

## Verification

```sh
cargo check --locked -p kernel-client --target riscv64gc-unknown-none-elf
cargo test --locked -p kernel-client --test call --target x86_64-unknown-linux-gnu
```

Tests exercise `invoke_with` with recording transports, including a local
ReserveRequest snapshot, scoped descriptors and typed replies. They do not
execute named native traps; RV64 compilation checks those wrappers. The Kernel
guest suite executes actual traps separately. Read API's
[compatibility and completion rules](../api/README.md#source-migration-and-abi-compatibility)
before updating a consumer pin or reusing a Page/IOVA.
