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
| `model::unit` | ProgramKind, UnitTarget, control authorities, creation descriptors, queries and stable results |
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

This branch implements Issue #42 in stages. Pend, Sign, Unit, Room and versioned
transport have migrated; Cell still uses its prior Attach/Detach and Begin/End
contracts until the Page freezing replacement is complete.

Call numbers are `(version << 48) | (domain << 32) | operation`; version is 1.
Version zero and unknown versions are rejected before domain dispatch. All byte
descriptors use fixed little-endian 64-bit words and checked tags/reserved fields.
Boot capsule format 2 declares the matching ABI in byte 11; old executable
capsules are rejected before launch. The resource ledger is version 3.

| Domain | Current operations |
| --- | --- |
| Pend | Accord, Revoke, Narrow, Return, Inspect, Collect |
| Clone View | Create, Delete, Push, Pull, Peek |
| Share View | Create, Delete, Open, Shut |
| Sign | Create, Delete, Listen, Trigger, Consume, Attach, Detach, Accord, Revoke, Return |
| Unit | Create, Delete, Resume, Freeze, Stop, Exit, Finish, Inspect, Accord, Revoke, Return |
| Room | Yield, SleepUntil, WaitValue, WakeValue, Clock |
| Debug | Put, Get |

Pend Accord snapshots a 64-byte request. Both Accord and Narrow interpret a
Pages range relative to the immediate parent's/current effective window.
Narrow retains identity, rejects live descendants with Busy, withdraws old CPU
access before committing a smaller window, and refuses outstanding external
responsibilities. The 88-byte PendInfo includes the effective source window.
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
Attach watches authorized sources. Accord can only reduce rights; Revoke and
Return invalidate the entire branch and wake listeners to revalidate. Delegation
depth is bounded at 64; exhausted sequences/handles fail instead of wrapping.
Delete requires endpoint ownership; returning an owner handle does not transfer
ownership. Owner death still closes its endpoints.

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

Dedicated Join/ReadResult, SelfId/Sire/Creator/Scan, generic key Wait/Wake,
Chrono/Ticks, Control/Backtrace and Debug SetTrace have been removed. Inspect,
Sign routing and Room Clock provide their retained core capabilities. Kernel
crash backtraces and bounded Debug Put/Get remain available.

Consumers must rebuild together with this API and regenerate capsules/ledgers.
Upper-layer migration is outside this Kernel change.

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
