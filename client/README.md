# Kernel Client

`kernel-client` is a `no_std` library for executing Kernel calls. It depends on
`kernel-api` and the declared error type, and needs no allocator or runtime state.

| Module | Responsibility |
| --- | --- |
| `call::{memory,unit,room,...}` | Named typed calls, including terminal `room::reap` |
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

Named signatures are declared in `src/call/definitions.rs`. The local macro only
removes execution boilerplate and does not define ABI values or generate service
protocols. Each domain imports the types it uses. All 66 returning call signatures
are retained. `call::room::reap(reason, note, len)` passes the three ABI fields
directly; callers provide a valid note buffer and obey `abi::call::room::NOTE_MAX`.
The old root `room::park(Duration)` and `room::reap(reason, Option<&str>)` adapters
have been removed.

```sh
cargo check --locked -p kernel-client --target riscv64gc-unknown-none-elf
cargo test --locked -p kernel-client --test call --target x86_64-unknown-linux-gnu
```

Tests exercise public `invoke_with` with a recording transport. They do not
execute the named native-trap wrappers; RV64 compilation checks those wrappers.
Native call
execution is verified by the consuming integration project. See the API
[compatibility notes](../api/README.md#source-migration-and-abi-compatibility)
before updating a consumer's Kernel pin.
