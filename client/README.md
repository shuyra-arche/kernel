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
Push/Pull use CellHandoff, Publish/Retract/Protect use CellPosition, Delete uses
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

ABI v5 creation calls take an explicit None-target Pend license. Sign handles are
now PendIds; use Pend Accord/Narrow/Revoke/Return with Operations::Sign instead
of the removed Sign authorization calls. SignId is descriptive only. Boot Ledger
v5 provides Name::Call(Call::Create); managers must explicitly delegate creation
permissions to new Tasks. Rebuild all users and capsule ABI declarations together.

`call::flat::{create, inspect, slice, delete}` uses a Flat Pend; `slice` returns a
checked SlotId. `call::nest` creates and observes Internal/External organizations;
External `attach` requires a separate Port Pend (`call::port::inspect`). Team
creation may consume an owned Internal Nest root through `UnitCreate::Team::nest`.
`Sign::attach` additionally takes a generation: exact Memo generation for
MappingPublished/Retired, zero for other sources. Use NestFault rather than the
retired MappingFault. See the API operation matrix and ABI v3 migration table.

`flat::create(license, size, align, level)` selects physical Cell geometry.
Use `Allocation::at_level` for Cell Create. `cell::split(slot)` and
`cell::merge(slot)` consume the old SlotId only on success and return its
replacement. They require an exclusive complete contiguous collection; mapping
leaf changes remain separate. ABI v5 requires rebuilding consumer capsules.
