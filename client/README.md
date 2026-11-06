# Kernel Client

Typed calls live under `call::{memory,unit,room,...}`. Their request types and
return decoding belong to canonical `kernel-api-env`; `ecall::trap` owns the
RV64 `ebreak` register boundary. Root `memory`, `unit`, and `room` expose the
reusable memory, heap, TLS and task mechanisms.

Mechanisms route effects through `crate::call`; raw memory calls must not recurse
into root `memory` wrappers. Platform Runtime owns whole-product startup and
exit policy.

Client's transport is the Kernel register boundary. It provides no generic
frame transport, service dispatch or request/reply protocol. Memory helpers use
the concrete API descriptor encoders directly; no generic byte-codec dependency
or Weave adapter is required. The root Env re-export follows the narrowed
[API surface](../api/README.md#compatibility-and-validation).

`call` exposes only the named call modules. Import API types directly from Env
or the Client root; `call` no longer re-exports the Env API. Internal mechanisms
use Env for types and `crate::call` for execution.

`invoke::invoke_with(call, transport)` performs packing, one supplied register
transport invocation, and API-owned result decoding. `unsafe invoke::invoke(call)`
selects the native trap and requires the ABI operation's execution-context,
pointer, and resource-lifetime obligations to hold.

Named call signatures are Client-owned declarations in `src/call/definitions.rs`.
The local macro in `src/call/macros.rs` removes wrapper boilerplate without
defining slots, register layouts, or failure codes. Operations return their own
success/error types; there is no domain return-enum extraction. Infallible
wrappers eliminate `Infallible` through an exhaustive match. All 66 existing generated entry points keep their public
signatures. `room::reap` constructs the terminal `Reap` operation and uses `invoke_terminal`.

Client installs no `_start`, startup argument globals, panic handler, or global
allocator. `memory::HeapAllocator` is a stateless registration handle for the
one existing Talc heap. The heap table, TLS table and detached-result registry
remain the sole authorities for their original responsibilities.

`__private` is the runtime adaptation contract:

```rust,ignore
unsafe fn initialize_tls() -> MemoryResult<Tls>;
unsafe fn retire_tls(token: Tls) -> Cleanup; // Complete | Deferred
```

Initialization installs `tp` only after successful setup. `Tls` is a unique,
task-local slot token with no automatic Drop effect. Retirement clears `tp`
before detaching; failed work stays in the existing table and returns Deferred.
Main and closure tasks use the same mechanism. A runtime must finish all TLS
users before retirement, then use a terminal path that needs no TLS. The room
terminal trap faults if it unexpectedly returns, without invoking panic again.

Legacy startup adapters may instead retain the Caller mapping through their
existing Kernel terminal path. Native task termination owns that mapping's
cleanup; a native terminal observation permits retiring local table bookkeeping.
The existing TLS sweeper scans only while cleanup is owed or a reference release
is pending, so it does not guarantee reclaiming an otherwise retained Live slot
before the address space ends. Dropping the token does not clear `tp`, remove
the mapping, or discharge the table's responsibility. This allowance preserves
legacy startup and does not select a new Runtime exit policy.

`unit::observe(target, wait)` always uses non-consuming native observation and
does not clear TLS. Foreign Denied never establishes death. The private closure
registry retains its original same-Team, self-created task prerequisite for
interpreting a pruned member as terminal. Native Reaped permits retiring local
bookkeeping; Server can still have deferred physical cleanup.

Build from the repository root with:

```sh
cargo check --locked -p kernel-client --target riscv64gc-unknown-none-elf
```

Client shares the root workspace, lockfile and source version. `tests/calls.rs`
uses the same Client signature declarations with an explicit recording transport
to check packing, failure handling, high-bit infallible values, and three-word
returns. It performs no native trap. Runtime execution is validated by the
consuming system; see the root [README](../README.md).
