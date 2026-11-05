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

## Server integration and provenance

The candidate is based on Kernel main `8a2c5fb9263fb1b42b2088dbb88ddcb37e1e7348`.
Its ABI tree comes from the already merged canonical API checkpoint
`18bba00ce3dd722ee50d5a4acd00a642debe8223`. The source types and wire codec match
production `env@d3000253cd5d13d466e91fafd4dbb27a5ec5aaae` and
`wire@0ff77dc4573fa2620d2fe995a18c01949d9c7e57`. Production
`mold@f52501f4c91b686eded8b4bcd743b4e96084b7f9` adds its old execution-bound
entry macro; that entry remains excluded. Its frame comment update is retained.
The effect/decoder split in this candidate is intentional.

Root Server and `kernel-allocator` now use `kernel-api-env` directly.
`iommu-hold@90f59ef91d3c4c157fb78bb691991ee7abe4f175` comes from the canonical
`driver-iommu` repository and directly pins the public `kernel-api-env`. The root
manifest patches that API package to `api/env`, unifying its Git identity with
Server's checkout identity. It no longer patches legacy `env`. The resolved
Server graph has one `kernel-api-env`, no legacy `env`/wire definitions, and no
Client dependency. Server dispatch and scheduling source are unchanged.

External product repositories and old effectful `env` consumers have not been
migrated. They must move typed calls to Client in a separately validated
integration; the compatibility adapter is deliberately type/codec-only.

## Validation

With the session's nightly toolchain:

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
