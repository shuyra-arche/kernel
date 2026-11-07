# Kernel API

`kernel-api` is one `no_std` Cargo package. It owns the public Kernel contracts;
Client executes them and Server implements them. It requires no heap, startup
code or runtime state. There is no Env package or compatibility facade.

## Public modules

| Path | Responsibility |
| --- | --- |
| `address` | PhysAddr, VirtAddr, PageIndex, PageOffset, PageCount and page geometry constants |
| `model::identity` | TaskId, TeamId, PendId and Mark |
| `model::cell` | FlatReference, MappingId, allocation, reference ranges, Position and Attachment |
| `model::modifiable` | Modifiable: shared read or exclusive write access |
| `model::unit` | ProgramKind, UnitTarget, control authorities, task exit, result and join values |
| `model::{view,pend,clone_view,sign}` | View identity, Pend state, Clone queues and Sign routing |
| `model::{wait,exit}` | Waiting and exit vocabulary |
| `abi::call` | Operation declarations, domain failures and aggregate dispatch |
| `abi::operation` | Operation, RegisterCall, KernelCall and TerminalCall traits |
| `abi::register` | Checked request packing and unpacking, including Wire and Decode |
| `abi::reply` | Success register decoding and the Sign reply contract |
| `abi::layout` | Concrete caller-memory descriptor codecs |
| `abi::error` | Common failure representation and dispatch errors |
| `boot` | Initial-task arguments, Capsule, resource Ledger and initrd preamble |

The crate root exposes these four domains directly. Import names from their
owning modules; neither API nor Client flattens the whole vocabulary into a root
prelude. Model values install no allocator, locks, TLS or task cleanup. Concrete
descriptor methods remain inherent methods on their types; their implementations
live in `abi::layout`.

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

Call classes and operation slots are explicit 32-bit values. The macro checks
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

This is an intentional model/ABI break with no legacy aliases:

| Old contract | Current contract |
| --- | --- |
| Hole / Pole | Clone / Share View, addressed through PendId |
| Pie / generic Grant | Pend borrow tree with Modifiable No or Yes |
| FETCH/STORE/VEST/ONLY permissions | Modifiable for data; ControlId for administration |
| Nole / Tole / View-local Ring and Hush | Sign Listen/Trigger/Route, retained state and checked delegation |
| Nest View-prefixed handles | MappingId / MappingSelection / MappingControl |
| Seven-word resource inspection | Eight-word PendInfo including ViewId and PendStatus |
| All boot entries typed as data authority | Ledger version 2, ResourceKind-qualified raw handles |
| Implicit writable bootstrap handoff | Spawn execution, then explicit Pend Accord before starting the child |
| Implicit same-Team control | Explicit Task controller; membership does not grant peer control |

Cell operations use `abi::call::cell::CellCall` and `model::cell`; the old
MemoryCall/model::memory paths are removed. VirtAddr moves from model::identity
to address. Slot 10 (Transfer) is retired: Push (15) and Pull (16) take a Position
descriptor and optional change receipt. Begin (17) / End (18) delimit admission
changes across calls. Memo Pull uses FlatReference::NONE and zero destination
page; withdrawal does not mint ownership. Receipts are task-local authorization
objects, not Page IDs or pointers.

Old Attach slot 12 is retired. Slot 19 takes MappingTarget plus a pointer to one
72-byte Attachment containing ReferenceRange, Placement, access, MappingControl
and optional change receipt. Nonzero receipts are accepted for I/O publication;
CPU supply under a change uses Push. Detach (13), Inspect (14), class 2 and their
error encodings are unchanged. The API codec rejects malformed descriptors;
server authority checks remain mandatory. Client callers must marshal these
new descriptors and rebuild with the matching API.

Unchanged operations keep their slot numbers; removed operations are rejected.
Do not infer compatibility from a matching call class: permission values and
several layouts have deliberately changed. Build requires the caller's own
TaskId as sire. Unit Return, ReadResult and Team Finish separate result data,
execution exit, stable cohort completion and eventual resource reclamation.

Publish this provider first. Migrate consumers together to matching API/Client
revisions and regenerate boot ledgers. Previously compiled binaries are not
compatible. Full upper-layer migration is outside this Kernel change.

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
