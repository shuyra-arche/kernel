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
| `env` / `wire` in `compat` | Re-export adapters; no second type definitions |
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
dependency. Existing Server dispatch stays in the root Kernel implementation;
no empty Server directory or new cluster is introduced.

## Dependency identity

Root Server and allocator consume `kernel-api-env` directly. The root manifest
patches the API dependency of `iommu-hold` to `api/env` so the resolved Server
graph has one canonical owner. Consumer workspaces must coordinate their own
source identity; a path and Git copy of the same types are distinct owners.
Compatibility adapters are type/codec-only. Effectful calls use Client.

## Validation

Use the repository's `rust-toolchain.toml`:

```sh
cargo test --manifest-path api/Cargo.toml --workspace --target x86_64-unknown-linux-gnu
cargo build --lib --bin shuyra --target riscv64gc-unknown-none-elf
cargo tree --target riscv64gc-unknown-none-elf -i kernel-api-env
cargo check --manifest-path client/Cargo.toml --target riscv64gc-unknown-none-elf
sh client/tests/check-boundaries.sh
```

The API workspace retains ABI, identity, codec and macro tests. Client's contract
fixture covers dependency renaming, shared handles, narrow/error/wide returns,
and full-width infallible clock returns. Compile-fail probes prove API has no
trap or enum call and TLS tokens cannot be copied or sent. Client mechanism
fixtures are documented in `../client/README.md`.

These checks establish compilation, type identity and host mechanism behavior.
They do not establish QEMU execution or a whole-product consumer migration.
