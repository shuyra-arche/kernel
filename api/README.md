# Kernel API

`kernel-api` is one `no_std` Cargo package. It owns the public Kernel contracts;
Client executes them and Server implements them. It requires no heap, startup
code or runtime state. There is no Env package or compatibility facade.

## Public modules

| Path | Responsibility |
| --- | --- |
| `model::identity` | TaskId, TeamId, PieToken, VirtAddr and Mark |
| `model::memory` | FlatReference, ViewId, allocation, reference ranges and placement |
| `model::permission` | Permission, Access and Policy |
| `model::unit` | ProgramKind, UnitTarget, task exit and join values |
| `model::{pie,hole,pole,tole}` | Resource descriptions and event conditions |
| `model::{wait,exit}` | Waiting and exit vocabulary |
| `abi::call` | Operation declarations, domain failures and aggregate dispatch |
| `abi::operation` | Operation, RegisterCall, KernelCall and TerminalCall traits |
| `abi::register` | Checked request packing and unpacking, including Wire and Decode |
| `abi::reply` | Success register decoding and the Tole reply contract |
| `abi::layout` | Concrete caller-memory descriptor codecs |
| `abi::error` | Common failure representation and dispatch errors |
| `boot` | Initial-task arguments, Capsule, resource Ledger and initrd preamble |

The crate root exposes these three domains directly. Import names from their
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

This is an intentional source break with no forwarding packages or aliases:

| Previous use | Current use |
| --- | --- |
| `kernel-api-env` / dependency alias `env` | `kernel-api` |
| `kernel_api::env` or Client root type exports | `kernel_api::{model,abi,boot}` and explicit imports |
| `env::abi::call` | `kernel_api::abi::call` |
| `env::{Operation,RegisterCall,KernelCall,TerminalCall}` | `kernel_api::abi::operation` |
| `env::wire` | Types in `model`; codecs in `abi::{register,reply}` |
| `env::ledger::{entry,name}` | `kernel_api::boot::ledger` |
| `env::ledger::{args,capsule}` | `kernel_api::boot::{args,capsule}` |
| `env::ledger::manifest` | Removed; deployment format belongs to Vanllia |
| Client heap, mapping ownership, closure tasks, locks and TLS | Removed; language runtime/std owns these mechanisms |

All 67 operation numbers, register layouts, failure codes and existing concrete
byte formats remain unchanged. `call::room::reap(reason, note, len)` now exposes
the terminal ABI directly; there is no duration/note adaptation or TLS cleanup.

Publish this provider before migrating consumers. External consumers must update
source imports, replace their use of removed runtime/deployment helpers, and pin
one full Kernel commit for both API and Client. Older pinned revisions remain
independent. This repository does not update external consumer pins.

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
