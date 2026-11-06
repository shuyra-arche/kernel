# Kernel Client

Typed calls live under `call::{memory,unit,room,...}`. Their request types and
return decoding belong to canonical `kernel-api-env`; `ecall::trap` owns the
RV64 `ebreak` register boundary. Root `memory`, `unit`, and `room` expose the
reusable memory, heap, TLS and task mechanisms.

Mechanisms route effects through `crate::call`; raw memory calls must not recurse
into root `memory` wrappers. Platform Runtime owns whole-product startup and
exit policy.

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

Client shares the root workspace, lockfile and source version. Legacy host
fixtures and boundary scripts have been removed. Runtime execution is validated
by the consuming system; see the root [README](../README.md).
