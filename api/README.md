# Kernel API and Client boundary

Kernel owns one canonical ABI. Server consumes it directly; Client generates
typed calls from those same declarations. Platform Runtime remains a separate
consumer of Client lifecycle mechanisms.

| Package | Responsibility |
| --- | --- |
| `kernel-api-env` | Call payloads, slot/argument codecs, pure `decode_return`, handle and error vocabulary, permissions, ledger and marks |
| `kernel-api-wire` | Byte codec traits, frames and message contracts |
| `kernel-api-macros` | Codec/contract derives and the hidden typed Client generator |
| `kernel-api` | Re-export umbrella |
| `kernel-client` in `../client` | Transport, typed free functions and reusable memory/task mechanisms |

`kernel-api-env` performs no trap and exposes no `.call()` method. A declaration
such as `MemoryCall` owns `slot`, `pack`, `from_wire`, and `decode_return`.
Its hidden generator instantiates `kernel_client::call::memory::release`, which
only constructs the request, invokes Client transport, then extracts the typed
value from API's decoder. Return decoding is implemented once. Infallible
variants retain the full return word, including high bits used by clocks.

Generated owner references use `$crate`; the caller supplies only the transport
path. This also works when the dependency is renamed. The API macro package has
no entry generator, startup, panic handler, allocator registration, or Client
dependency. Server dispatch lives in `../server/`. All packages belong to the root Cargo
workspace.

## Dependency identity

Server and allocator consume `kernel-api-env` directly. The root manifest
patches the API dependency of `iommu-hold` to `api/env` so the resolved Server
graph has one canonical owner. Consumer workspaces must coordinate their own
source identity; a path and Git copy of the same types are distinct owners.
Effectful calls use Client.

## Build

From the repository root:

```sh
cargo check --locked -p kernel-api -p kernel-api-env -p kernel-api-wire -p kernel-api-macros --target riscv64gc-unknown-none-elf
cargo tree --locked --target riscv64gc-unknown-none-elf -i kernel-api-env
```

Legacy tests and their compatibility-only adapters have been removed. The root
[README](../README.md) describes compilation checks and source version releases.
