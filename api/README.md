# kernel-api PR2 candidate

This directory is the isolated PR2 proof for one canonical API package set under
Kernel. It is not a production consumer cutover. The original `env`, `wire`, and
`mold` crates remain baseline outside this PR; no Source, Router, RTC, QEMU, or
production consumer/lock file is changed here.

## Owned packages

| Path | Package / lib | Owns |
| --- | --- | --- |
| `api` | `kernel-api` / `kernel_api` | Umbrella only: `pub use kernel_api_env as env; pub use kernel_api_wire as wire;`. No definitions. |
| `api/env` | `kernel-api-env` / `kernel_api_env` | Canonical env ABI, moved from the original `env` source: call payloads, `Wire`/`Decode`, permissions, `Wait`, exit codes, handles, ledger, marks, `ecall::{trap, make_fail, FailCode}`, and the register vocabulary module `env::wire`. |
| `api/wire` | `kernel-api-wire` / `kernel_api_wire` | Canonical byte codec, moved from the original `wire` source: `Contract`, `Message`, `Field`, `Span`, tail helpers, and `OK`. |
| `api/macros` | `kernel-api-macros` / `kernel_api_macros` | The `mold` codec/contract macro implementation: `Frame`, `Envcall`, `Fail`, `WireCodes`, `interface`, and `contract`. **`entry` is excluded.** |
| `api/compat/env` | `env` | Re-export-only compatibility facade over `kernel-api-env`. |
| `api/compat/wire` | `wire` | Re-export-only compatibility facade over `kernel-api-wire`. |

The compatibility facades depend directly on their owner packages, never on the
umbrella. They exist only for isolated identity and macro fixtures in this
workspace.

Dependency direction:

```text
kernel-api -> kernel-api-env, kernel-api-wire
kernel-api-env -> kernel-api-wire, kernel-api-macros, bitflags, erra
kernel-api-wire -> (no dependencies)
kernel-api-macros -> proc-macro2, quote, syn
env(compat) -> kernel-api-env
wire(compat) -> kernel-api-wire
```

No runtime edge points at the original `env`, `wire`, or `mold` repositories,
and no original crate appears in the new runtime graph.

## Preserved behaviour

Source is copied from the original `env`/`wire`/`mold` trees. Only crate-local
aliases and the absolute codec path in the migrated wire fixture were corrected:

- `api/env` keeps `extern crate self as env;`, so `Frame`'s default
  `::env::wire` path and `Envcall`'s `crate::wire`/`crate::ecall` expansion
  continue to point at the owned env crate.
- `api/env/src/wire/field.rs` still writes `::wire`; the owner package is
  declared under the dependency name `wire`, so no source path was rewritten.
- `api/macros` keeps the original generated paths, including `crate::wire`,
  `crate::ecall`, `::env`, and `::wire`. It does not export `entry`, does not
  contain `entry.rs`, and has no `execution` dependency.
- The wire host fixture uses `crate::wire` only because its local compatibility
  module is named `wire`; generated macro paths and observable codec behaviour
  are unchanged.

The original env test files are migrated into `api/env/tests` with a single
`use kernel_api_env as env;` alias so their original `env::...` paths remain
readable. The original wire host test is migrated into `api/wire/tests/host.rs`
with its local `env`/`wire` aliases intact. `api/tests/env_identity.rs` and
`api/tests/wire_identity.rs` now compare the compatibility facades with the
umbrella against the same single owner types. `api/tests/macro_contract.rs`
adds a positive `Frame`/`Message`/`Contract`/`WireCodes`/`interface` fixture.
The macro crate keeps only its inherited negative expansion tests for
`Envcall` and `interface`; `contract` negative coverage remains Root's
compile-fail probes, not an inline test module in the production macro file.

## Required release order

1. Publish the Kernel owner packages (`kernel-api-env`, `kernel-api-wire`,
   `kernel-api-macros`) at the full Kernel SHA. Then publish the external
   `env`/`wire` adapters pinning that same full Kernel SHA (the external repos
   have their own SHAs). Production cutover follows only after the separately
   reviewed macro/entry split.
2. Keep the old external `mold` in place for now. A proc-macro crate cannot
   directly re-export these proc macros while also retaining `entry`; defer the
   external `mold` wrapping/splitting to a separate PR that provides a regular
   facade plus a separate entry proc-macro owner. Do not claim the `mold`
   replacement is complete before that PR.
3. After that split lands, update the entire consumer closure atomically to the
   full Kernel SHA. This PR performs no production cutover.

A partial graph containing both an old owner definition and a new owner
definition must be rejected: two nominal `env`/`wire` definitions in one build
graph would break type identity. Do not run old and new owners side by side.

## Verification (Root owns Cargo)

Root regenerated the candidate lock for the migrated graph and runs Cargo. This
worker did not run Cargo and does not claim a pass.

Expected host checks from `api/`:

```sh
cargo test --target x86_64-unknown-linux-gnu
cargo check --lib --target riscv64gc-unknown-none-elf
cargo tree --target x86_64-unknown-linux-gnu
```

Expected properties for Root validation:

- umbrella `kernel_api::env` and `kernel_api::wire` are re-exports of the owner
  packages;
- compat `env`/`wire` and umbrella names resolve to the same owner types;
- migrated env tests and wire host codec tests retain their original assertions;
- the macro fixture compiles with compat `env`/`wire` aliases and no old crate;
- no `entry` symbol or `execution` edge is present in `kernel-api-macros`;
- the runtime dependency graph contains no original `env`, `wire`, or `mold`
  package.

Host tests do not prove a real syscall/trap path. Native QEMU and production
consumer verification are out of scope for this PR.
