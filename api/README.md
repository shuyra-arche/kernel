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

## Source migration and ABI compatibility

This is a breaking provider redesign of Cell, View, Pend, Sign, Unit and Room.
There are no compatibility aliases for retired operations.

Rust callers now match `OutOfMemory` for allocation failures. Each domain uses
its existing numeric ABI code for this variant.

Call numbers are `(version << 48) | (domain << 32) | operation`; version is 3.
Versions zero, one, two and unknown versions are rejected before domain dispatch. All byte
descriptors use fixed little-endian 64-bit words and checked tags/reserved fields.
Boot capsule format 2 declares the matching ABI in byte 11; old executable
capsules are rejected before launch. The resource ledger is version 4.

| Domain | Current operations |
| --- | --- |
| Cell | Create, Delete, Push, Pull, Publish, Retract, Protect, Inspect |
| Pend | Accord, Revoke, Narrow, Return, Inspect, Collect |
| Clone View | Create, Delete, Push, Pull, Peek |
| Share View | Create, Delete, Open, Shut |
| Sign | Create, Delete, Listen, Trigger, Consume, Attach, Detach |
| Unit | Create, Delete, Resume, Freeze, Stop, Exit, Finish, Inspect, Accord, Revoke, Return |
| Room | Yield, SleepUntil, WaitValue, WakeValue, Clock, ClockInfo |
| Debug | Put, Get |

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
| Create Memo | Authorized Slot window or relative Pend window; destination control | Reserves address positions; Slot source starts Vacant, Pend source supplies Ready associations; never publishes |
| Delete Slot | Calling Task's SlotId | Releases that owning handle; Memo and hardware responsibilities survive |
| Delete Memo | Mapping controller, explicitly permitted Space members, or authorized I/O target controller | Requires all publication withdrawn and fenced; deletes associations and freezes; Busy preserves the record |
| Push / Pull | Source access plus destination control; actual Slot ownership for any owning transfer | Same endpoint matrix, expressed from giver/taker; no PTE or IOMMU publication effect |
| Publish | Valid source authority and qualified Ready Memo | Starts hardware publication of the associated Page, not whatever now occupies its source Slot; Inspect distinguishes Publishing from Published |
| Retract | Qualified Published Memo (I/O also Publishing) and mapping control | Closes Page admission before withdrawal; accepted I/O request is not completion; query the original object |
| Protect | Qualified CPU Memo and access within its original ceiling | Changes one position's access and generation; no new association or ownership; I/O protection is immutable for its one-shot binding |
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
that memory has been freed. Task death retains hardware records through completion;
a settled I/O record expires after its creator dies, and later lookup fails.

I/O currently supports one 4 KiB Page, exact IOVA, Caller control, and READ or
READ|WRITE. The QEMU EDU route is deliberately one-shot: a settled route cannot
be republished (`NotTransferable`), even though the logical Memo remains
inspectable/deletable. Admission verifies the captured Page against its actual
source; an unrelated replacement Slot cannot silently substitute for it. This
contract makes no promise about unvalidated physical-device quiescence.

Public Acquire/Release, Attach/Detach, Begin/End, Traverse, Position, Attachment
and general Retain authority are removed. Their replacements are Cell objects,
qualified transitions and kernel-owned hardware retirement, not a public change
receipt or duplicate reference lifecycle.

### Authority and control contracts

Pend Accord snapshots an 88-byte request; Narrow snapshots a 64-byte request.
Both carry typed operation permissions, independent Modifiable/Distributable
limits, and a checked range. Derivation requires Distributable; neither
derivation nor narrowing can add operations or change the target family.
Both Accord and Narrow interpret a
Pages range relative to the immediate parent's/current effective window.
Narrow retains identity, rejects live descendants with Busy, withdraws old CPU
access before committing a smaller window, and refuses outstanding external
responsibilities. The 112-byte PendInfo identifies its target and includes the
effective permissions and source window. FlatId and NestId are stable object
identities; they grant no access on their own.
None and Sign use the same Pend ledger and derivation tree as Views. The target
vocabulary also reserves Flat, Nest and Port; runtime registration of these three
is the remaining #83 work. None is object-free call permission: its Modifiable
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

Creation grants all supported operations to the new View's root. Boot grants
are narrower: fault queues allow PULL/PEEK, memory resources allow
OPEN/SHUT/ASSOCIATE. Neither permits deleting Kernel-owned resources. Accord
requires an explicit operation subset and Distributable Yes. Return and Revoke
continue to follow ownership of the authorization branch and need no extra
operation grant. Removing data permissions first withdraws existing mappings;
retained Slot references revalidate the same authority before association.
Inspect/Collect return identity and state, never a reusable authorization proof.
Return and Revoke restore parent access only after descendant and hardware
responsibilities have settled. Busy retains cleanup responsibility.

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
Team creation requires Build authority; Supervisor creation additionally requires
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
Task Exit publishes a value plus a bounded diagnostic note; the first committed
termination wins. Exit is not proof of Page/DMA reclamation. Unit Accord/Revoke/
Return operate on independent administrative authorities with revocable ancestry.

| Call | Required authority / state | Completion or retained effect |
| --- | --- | --- |
| Clone Create | None CLONE license | New queue and root Pend |
| Share Create | None SHARE license and authorized Slot/source window | New View root or narrower branch; source qualification and borrowing still apply |
| Clone/Share Delete | Matching target DELETE | Closes object; unfinished borrows and hardware obligations remain |
| Clone Push | Clone PUSH and Modifiable Yes; queue limits | Enqueues one copied message or fails; no implicit retry |
| Clone Pull / Peek | Clone PULL / PEEK | Pull delivers message authority; Peek observes length/sender/count without consuming |
| Share Open / Shut | Share OPEN / SHUT; range, Modifiable and borrowing | Opens/closes this caller's access using the common Nest publication machinery; Shut neither returns Pend nor deletes View |
| Pend Accord | Delegable parent and non-expanding relative window/access | New local/remote branch; failed preparation does not publish partial authority |
| Pend Revoke / Return | Direct grantor / holder | Retires branch; Busy keeps obligations and parent access suspended |
| Pend Narrow | Holder; no live descendants or external obligations | Same identity with reduced access/window after old CPU withdrawal |
| Pend Inspect / Collect | Caller-local table | Single/paged discovery; stable View identity allows comparison, never authorization |
| Sign Create | None SIGN license | New endpoint and root Sign Pend |
| Sign Delete | Sign DELETE | Closes endpoint and wakes all listeners |
| Sign Listen | Sign LISTEN; Poll/At/Infinite | Observes direct event or routed source; does not consume either |
| Sign Trigger / Consume | Sign TRIGGER / CONSUME; empty/current sequence respectively | Sets a new sequenced event / clears exactly that event; stale consume fails |
| Sign Attach / Detach | Sign ROUTE plus source observation authority | Adds/removes a route; readiness is revalidated while waiting |
| Unit Create | Own Team or controlled constructing child; Build for Team | Dormant Task or empty Team; explicit Resume begins execution |
| Unit Delete | Authorized observer/manager and completed reclamation | Removes caller's Task record or retired Team; unfinished cleanup is not discarded |
| Unit Resume / Freeze | Controller or ancestor manager | Admits/suspends execution; does not alter results or transfer authority |
| Unit Stop / Exit | Controller / executing Task | Requests termination / terminates with result; first committed termination wins |
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
Provider and consumer revisions must move together. The following migration is
incompatible with both v1 and the interim v2; old call numbers fail before dispatch.

| Previous interface | ABI v3 / Ledger v4 | Consumer migration |
| --- | --- | --- |
| Clone/Share Create without permission | Leading `license: PendId` with None CLONE/SHARE | Read boot Create license or receive it through Pend Accord |
| Sign Create returning SignId | None SIGN license; returns PendId | Use Pend Inspect for descriptive SignId |
| Sign calls and SignSource::Sign using SignId | Same operations with Sign-target PendId | Resolve only the caller's actual Pend |
| Sign Accord/Return/Revoke at slots 7–9 | Removed; use Pend Accord/Return/Revoke | Encode Sign operations in Limits; do not reinterpret old handles |
| Cell Create | Leading license; Slot requires None FLAT | Supply explicit license for allocation; Memo authority comes from source/destination |
| Separate Sign table | One Task Pend ledger | Collect includes None/Sign; inspect Target before using a token |
| Pend descriptors in v1 | Target-tagged Limits, 88-byte Accord, 64-byte Narrow, 112-byte Info | Rebuild byte snapshots; unknown bits and mismatched target tags reject |
| Boot interrupt handle in Ledger v3 | Sign-target Pend in Ledger v4 | Rebuild capsule ABI byte and ledger parser; cannot cast an old Sign handle |

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
