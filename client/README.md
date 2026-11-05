# Kernel Client

Typed calls live under `call::{memory,unit,room,...}`. Their request types and
return decoding belong to canonical `kernel-api-env`; `ecall::trap` owns the
RV64 `ebreak` register boundary. Root `memory`, `unit`, and `room` expose the
reusable mechanisms imported from production
`execution@1b333a28bc4c34beaea3f7986a9bb6e8ab2db414`.

The imported Rust mechanisms now route their former `env` effect calls through
`crate::call`; raw memory calls cannot recurse into root `memory` wrappers.
External product call sites are unchanged. Platform Runtime implementation and
whole-product startup/exit policy are outside this API checkpoint.

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

`unit::observe(target, wait)` always uses non-consuming native observation and
does not clear TLS. Foreign Denied never establishes death. The private closure
registry retains its original same-Team, self-created task prerequisite for
interpreting a pruned member as terminal. Native Reaped permits retiring local
bookkeeping; Server can still have deferred physical cleanup.

Host fixtures import the actual production mechanism source:

```sh
cargo test --manifest-path client/tests/Cargo.toml --target x86_64-unknown-linux-gnu
cargo test --manifest-path client/tests/unit/Cargo.toml --target x86_64-unknown-linux-gnu
cargo test --manifest-path client/tests/lifecycle/Cargo.toml --target x86_64-unknown-linux-gnu
cargo test --manifest-path client/tests/contracts/Cargo.toml --target x86_64-unknown-linux-gnu
sh client/tests/check-boundaries.sh
```

The 13 original memory/heap tests retain allocation, stale snapshot, table-full,
failure and retry assertions. The task fixture retains result publication,
spawn/embark rollback, pruned tasks and abandoned results. The lifecycle fixture
checks initialization failure, duplicate installation, failed detach/release,
owner-specific retry, foreign Denied and real native terminal observation.
Startup argument testing belongs to Platform Runtime after removal of the old
global argument mechanism. RV64 Client compilation passes; real traps and
whole-product QEMU integration are outside this checkpoint.
