# Kernel Client

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
`unsafe invoke(call)` selects the native trap; its caller must satisfy the
operation's execution context, pointer validity and resource lifetime contract.
`invoke_terminal(call)` enters a non-returning fault path if the terminal trap
unexpectedly returns; it uses neither TLS nor a panic handler.

Each domain declares its named signatures and imports in its own module under
`src/call`. The shared macro executes typed requests defined in Kernel API.
`unit::exit` takes a result, reason and readable diagnostic note buffer.
The buffer length is bounded by `abi::call::unit::NOTE_MAX`.
`deadline_after(Duration)` converts a relative timeout once using Room Clock;
reuse the returned Wait across retries to avoid extending the deadline.

`call::pend::accord(request)` takes the address of `AccordRequest::to_bytes()`;
keep that 88-byte buffer readable until the call returns. Use AccordTarget::Caller
for local derivation or AccordTarget::Task for delegation. PendRange offsets are
relative to the parent window. `call::pend::narrow(request)` reads the 64-byte
NarrowRequest descriptor. Both requests carry typed operation permissions and
independent mutability/distribution limits. `call::pend::return_(token)` relinquishes the
holder's branch through Kernel retirement; Busy means cleanup is still pending.
The old `derive`, `same`, `release` and ReleaseMode interfaces are removed.

Cell calls take fixed descriptors from `model::cell`: Create uses CellCreate,
Push/Pull use CellHandoff `{ local, remote }`, Insert uses CellAssociation,
Remove/Publish/Retract/Protect use CellPosition, Delete uses
CellObject, and Inspect writes CellInfo. Use their explicit byte codecs. Inspect
supplies the generation for the next transition; preserve the absolute deadline
when retrying Busy. Native calls do not hide retries, publication or ownership.

```sh
cargo check --locked -p kernel-client --target riscv64gc-unknown-none-elf
cargo test --locked -p kernel-client --test call --target x86_64-unknown-linux-gnu
```

Tests exercise public `invoke_with` with a recording transport. They do not
execute the named native-trap wrappers; RV64 compilation checks those wrappers.
The Kernel guest suite also executes real traps for Cell, Pend, Sign, Unit and Room. See the API
[compatibility notes](../api/README.md#source-migration-and-abi-compatibility)
before updating a consumer's Kernel pin.

Resource creation calls take an explicit None-target Pend license. Sign handles are
PendIds; use Pend Accord/Narrow/Revoke/Return with Operations::Sign instead
of the removed Sign authorization calls. SignId is descriptive only. Boot Ledger
v9 provides Name::Call(Call::Create); managers must explicitly delegate creation
permissions to new Tasks. Rebuild all users and capsule ABI declarations together.

`call::unit::exit(value, reason, note, len, scope)` requires an explicit
`ExitScope`. Select `Task` for a single Task or `Team` from a non-realtime
member of a Team configured with `member_exit`. The typed client treats a
return as a terminal-call contract violation.

`call::flat::{create, inspect, slice, delete, block, allow}` uses a Flat Pend; `slice` returns a
checked SlotId. `call::nest::create(license)` creates an Internal Nest only.
Team creation may consume that owned root through `UnitCreate::Team::nest`.
External Nests instead arrive as authenticated boot grants named
`Name::Nest { controller, requester }`; these names describe topology, not authority.
There is no `call::port`, runtime external Attach/Detach or caller-supplied
requester. `nest::inspect` writes a 56-byte NestInfo without a Port identity.

Keep the External Nest root with its manager and delegate limited Pend branches
for actual users. Returning/revoking a worker branch retires its own Memos,
including unused reservations, without replacing the Nest or closing other live
branches. Set Distributable to No for workers that cannot delegate further.
A different Task can reserve Memos after the former Task's positions have all
retired and been removed. `nest::delete` requires the boot root to terminate
an External Nest. Busy retains the close until hardware settlement and Closed
is terminal. Live descendants, borrows and active publications constrain Narrow.

`Sign::attach` additionally takes a generation: exact Memo generation for
MappingPublished/Retired, zero for other sources. Use NestFault rather than the
retired MappingFault. Fault observations remain with the same Nest across Pend
handover; an old MappingId cannot name a later mapping at the same address.

`flat::create(license, size, align, level)` selects physical Cell geometry.
Use `Allocation::at_level` for Cell Create. `cell::split(slot)` and
`cell::merge(slot)` consume the old SlotId only on success and return its
replacement. They require an exclusive complete contiguous collection; mapping
leaf changes remain separate.

Hardware dispatch is read from Ledger v9 as an explicit `(View, Sign)` pair;
the pair layout introduced in v7 is unchanged. For an admitted device this is a
Share View Pend plus Sign Pend. Delegate the full writable View before the
non-distributable Listen/Consume Sign, rolling back View on failure before
resuming the recipient. Initialize the device, Listen and Consume the initial
notification; thereafter clear each device cause before consuming its sequence.
Return/Revoke either Pend closes delivery. Sign retains its slots and payloads
under the current ABI prefix; consumption does not synchronously wait for controller
completion. Raw device/configuration authority must be withdrawn separately
before reassigning a device; Pend distribution alone proves no DMA quiescence.

ABI 9 Memo objects carry a PendId. Store `CreatedCell::Memo { token, address }`,
then explicitly Accord Memo operations and position windows to another Task.
Push moves local to remote; Pull moves remote to local. Both require owning
Slots. Insert/Remove only change associations; they never transfer ownership.
Keep the exact Flat BLOCK branch while retrying Block. Allow reopens only its
own block and does not implicitly Publish. Read the API's completion rules
before reusing a Page or IOVA.

For global statistics, discover `Name::Memory` with `Entry::dispatch()`, Open
its Share and retain that mapping while reading `model::memory::SnapshotPage`.
`read()` is a bounded atomic attempt; None calls for a later retry. LISTEN and
CONSUME use ordinary Sign semantics; consume then recheck Snapshot.epoch to avoid
missing coalesced updates. A/D values are UNKNOWN. Statistics assist policy,
but cannot authorize a transfer or certify hardware retirement.
