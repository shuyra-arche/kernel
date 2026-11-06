# kernel

Ordinary execution does not record a kernel event log. Syscall, scheduler,
page-fault, task and boot paths contain no event ring or per-event host export.
Exception diagnostics and crash backtraces remain available. The optional
`semihosting` feature exports only explicit crash reports to `kernel-crash.json`;
integration-test verdicts use native task exit and reclaim receipts in System.

Kernel provider, canonical ABI and Client mechanisms. The root workspace owns
the boot/runtime mechanism and its tests; `api/` and `client/` are independent
workspaces. This repository is not a ledger Runtime Unit: it has
no UnitFile, server wrapper, image builder, Integration harness, firmware, or
scene script.

Source snapshot: `https://github.com/anreonyr/sqware.git` at
`672d8d2deafc8067c94b6af340a5f89c6a765c9c`, branch
`codex/shuyra-device-contracts-extraction`, public draft PR 45. The snapshot is
the former `source/kernel/` tree: the initial import contained 490 tracked
files, with only the root `Cargo.toml` adapted for the standalone workspace.
Subsequent changes include canonical ABI ownership, Client mechanisms and
removal of event logging. See [provenance](PROVENANCE.md) for the historical
import boundary and Git history for later changes. Internal `#[path]`/`include!`
projections stay inside this repository.

## Layout

- Root package `kernel` 0.1.0: `src/`, `build.rs`, `link.x`.
- `allocator/` - `kernel-allocator` mechanism package.
- [`api/`](api/README.md) - independent canonical ABI workspace: payloads,
  codecs, handles, macros and compatibility re-exports; no trap effects.
- [`client/`](client/README.md) - independent Client workspace: transport,
  typed calls and memory/task mechanisms; no startup, panic handler or global
  allocator registration.
- `tests/api/` - `kernel-test-api` shared control-wire package.
- `tests/embedded.rs` + `tests/health/` - native `embedded-test` health suite.
- `tests/{console-line,custody,dma-confirmation,export,frame-runs,pend,
  scheduler-dispatch,wait-block,weak-copies}/` - nine independent owned
  host workspaces. Their lockfiles are tracked; some nested `.gitignore` files
  retain historical `/Cargo.lock` rules.
- `tests/trace/` - historical lockfile and ignore file only; no runnable Cargo
  workspace remains after event-log removal.
- `tests/critical/` - independent verifier scripts.

`SOURCE-MANIFEST.tsv` records the initial 490-path import, including its original
SHA-256 values. It is historical evidence, not a checksum of the current tree.

Start with [CONTRIBUTING.md](CONTRIBUTING.md) for the complete workspace and
validation command map. API/Client boundary checks and RV64 builds are separate
from native execution acceptance.

## Toolchain and build

`rust-toolchain.toml` is copied from Source: nightly-2026-09-04. `Cargo.toml`
declares edition 2024 and resolver 3. `.cargo/config.toml` pins the RV64 target and the same three
rustflags as Source. It intentionally has no `runner` or `image` alias.

```sh
cargo metadata --locked --offline --format-version 1
cargo build --locked --target riscv64gc-unknown-none-elf --lib --bins
cargo build --locked --release --target riscv64gc-unknown-none-elf --lib --bins
```

The nine Kernel host suites are independent workspaces and must use the explicit host
target because the provider root pins RV64:

```sh
cargo test --manifest-path tests/<suite>/Cargo.toml \
  --locked --target x86_64-unknown-linux-gnu
```

The allocator package equivalent is `cargo test --manifest-path
allocator/Cargo.toml --locked --target x86_64-unknown-linux-gnu`.

Host tests are owned-host type/byte/ledger checks. They are not native Kernel
authentication or hardware-effect proof.

## External test tool

This provider does **not** bundle QEMU board arguments, runner scripts, or
firmware. Native health is run through an immutable public Integration tool
checkout, never linked into or projected into Cargo:

- Tool: `https://github.com/anreonyr/sqware.git`
- Revision: `672d8d2deafc8067c94b6af340a5f89c6a765c9c`
- Firmware `SBI.bin` SHA-256:
  `c53d58aa9ab17f95fa1af159f0bf05fad5c5622ebc0f1084463435fa6e5752b6`
- Evidence-local checkout: `/home/anreonyr/Develop/shuyra/evidence/pr23/harness-source`

Representative no-scene invocation from this provider root:

```sh
nu <PUBLIC-HARNESS>/scripts/qtest.nu --package kernel --scene '' -- \
  --verbose -- external_retention_orders_real_frames_and_isolation
```

The no-scene path does not call `cargo image`; board arguments and firmware come
from the pinned public harness. This is not an Integration-scene pass and does
not claim a self-contained firmware distribution.

## Status

The published repository has advanced beyond the initial provider snapshot.
Root Server consumes the canonical API, and Client owns reusable effectful
mechanisms. External product consumer migration and whole-product runtime
acceptance remain separate work; see [API status](api/README.md).
Historical provider verification is recorded in `evidence/pr23/provider`
outside this repository. Repository-local records are linked from
[CONTRIBUTING.md](CONTRIBUTING.md).
No license is added; the Source snapshot tracked no license.

Copied test documentation retains its original Source-relative command paths.
Use the provider-root commands and pinned external harness above for this
snapshot; the copied paths do not indicate bundled Integration scripts.
