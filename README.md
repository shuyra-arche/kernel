# Kernel

Shuyra's RV64 kernel, canonical ABI and typed client mechanisms. The kernel owns
scheduling, address spaces, tasks, permissions and resource lifetime. Product
composition and user-space startup policy belong to their consumers.

## Code map

| Entry | Responsibility |
| --- | --- |
| `src/`, `build.rs`, `link.x` | Kernel implementation and `shuyra` executable |
| `allocator/` | Physical allocation mechanisms |
| [`api/`](api/README.md) | Canonical types, call declarations and pure codecs |
| [`client/`](client/README.md) | Trap transport and reusable memory/task mechanisms |
| `tests/health/`, `tests/embedded.rs` | Target-side embedded health tests |
| `tests/*/Cargo.toml` | Independent host fixtures |
| [`tests/critical/`](tests/critical/README.md) | Halt-proof verifier and its host tests |

The root, API and Client have separate Cargo workspaces. Root workspace commands
do not include API, Client or independent host fixtures. Server consumes the API
without a Client dependency; Client does not install startup, a panic handler or
a global allocator.

## Build and verify

Use the toolchain and components in `rust-toolchain.toml`
(`nightly-2026-09-04`). The root Cargo configuration defaults to RV64, so host
commands must select an explicit host target. Examples assume Linux x86-64.

```sh
cargo build --locked --target riscv64gc-unknown-none-elf --lib --bins
cargo build --locked --release --target riscv64gc-unknown-none-elf --lib --bins
cargo test --manifest-path api/Cargo.toml --workspace --target x86_64-unknown-linux-gnu
cargo check --manifest-path client/Cargo.toml --target riscv64gc-unknown-none-elf
cargo test --manifest-path allocator/Cargo.toml --locked --target x86_64-unknown-linux-gnu
for suite in console-line custody dma-confirmation export frame-runs pend scheduler-dispatch wait-block weak-copies; do
  cargo test --manifest-path "tests/$suite/Cargo.toml" --locked --target x86_64-unknown-linux-gnu || exit 1
done
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tests/critical
```

Client fixture and negative compilation commands are in
[Client](client/README.md). Boundary probes require `sh`, `rg` and a populated
Cargo dependency cache. `tests/trace/` has no Cargo manifest and is not a suite.
Host fixtures and cross-compilation do not prove guest execution or hardware
resource reclamation.

## Native testing and limitations

This repository contains no QEMU runner or firmware. Product integration uses
[Vanllia](https://github.com/shuyra-arche/vanllia). The standalone embedded health
suite retains an external harness dependency: `anreonyr/sqware` commit
`672d8d2deafc8067c94b6af340a5f89c6a765c9c`, with `SBI.bin` SHA-256
`c53d58aa9ab17f95fa1af159f0bf05fad5c5622ebc0f1084463435fa6e5752b6`.
From this checkout, with `PUBLIC_HARNESS` pointing to that harness checkout:

```sh
nu "$PUBLIC_HARNESS/scripts/qtest.nu" --package kernel --scene '' -- \
  --verbose -- external_retention_orders_real_frames_and_isolation
```

Ordinary execution has no kernel event stream. Exceptions and crash backtraces
remain; optional `semihosting` exports explicit crash reports only. Integration
verdicts require native task exits and reclamation receipts from System.

The pinned Rust toolchain and native runner need fresh validation before a
runtime acceptance claim; prior checkpoints used a different nightly. See
[contribution rules](CONTRIBUTING.md) and [source attribution](PROVENANCE.md).
