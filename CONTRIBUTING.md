# Contributing and validation

Run commands from the repository root with the toolchain pinned in
`rust-toolchain.toml` (nightly-2026-09-04). Install that toolchain and its RV64
target before building. Host examples below use `x86_64-unknown-linux-gnu`;
use a supported host target explicitly because the root Cargo configuration
defaults to RV64. The boundary probe script currently requires that Linux host
target, a POSIX shell and `rg` (ripgrep).

## Ownership and workspaces

- Root `Cargo.toml`: Kernel Server, allocator and test control-wire package.
- `api/Cargo.toml`: canonical ABI, codec/macros and compatibility re-exports.
- `client/Cargo.toml`: Client transport and reusable lifecycle mechanisms.
- Host fixtures have independent manifests and lockfiles.

Keep these boundaries intact. A root `cargo test --workspace` is not an
aggregate test of API, Client or the independent host fixtures. Server uses
canonical API without depending on Client. Client does not register startup,
a panic handler or a global allocator. Runtime integration is separate work.

## Kernel host checks

```sh
cargo test --manifest-path allocator/Cargo.toml --locked --target x86_64-unknown-linux-gnu

for suite in console-line custody dma-confirmation export frame-runs pend scheduler-dispatch wait-block weak-copies; do
  cargo test --manifest-path "tests/$suite/Cargo.toml" --locked --target x86_64-unknown-linux-gnu || exit 1
done
```

`tests/trace/` contains only historical lock/ignore files and is not a test
workspace. Native health tests under `tests/health/` use the external harness
described below, not the host loop.

## API and Client checks

```sh
cargo test --manifest-path api/Cargo.toml --workspace --target x86_64-unknown-linux-gnu
cargo test --manifest-path client/tests/Cargo.toml --target x86_64-unknown-linux-gnu
cargo test --manifest-path client/tests/unit/Cargo.toml --target x86_64-unknown-linux-gnu
cargo test --manifest-path client/tests/lifecycle/Cargo.toml --target x86_64-unknown-linux-gnu
cargo test --manifest-path client/tests/contracts/Cargo.toml --target x86_64-unknown-linux-gnu
sh client/tests/check-boundaries.sh
```

The last command uses offline Cargo resolution. Populate its dependency cache
before running it in a fresh environment. It expects the forbidden API effects
and invalid TLS ownership examples to fail compilation with specific diagnostics;
an unrelated dependency or toolchain failure is not a successful probe.

## RV64 builds and dependency ownership

```sh
cargo metadata --locked --format-version 1
cargo build --locked --target riscv64gc-unknown-none-elf --lib --bins
cargo build --locked --release --target riscv64gc-unknown-none-elf --lib --bins
cargo check --manifest-path client/Cargo.toml --target riscv64gc-unknown-none-elf
cargo tree --target riscv64gc-unknown-none-elf -i kernel-api-env
```

Inspect the resolved Server graph for one canonical `kernel-api-env` owner and
no Client dependency. Do not treat successful cross-compilation or host fixtures
as proof of target execution, hardware effects or whole-product migration.

## Native acceptance and evidence

[README.md](README.md#external-test-tool) records the immutable public Integration
harness revision, firmware hash and representative no-scene health command.
This repository does not bundle that harness or its firmware. Follow that
document rather than copied Source-relative script paths in older test notes.
[Critical-path verifier notes](tests/critical/README.md) describe their separate
scenarios and requirements.

Existing checkpoint records:

- [API and Client validation](api/VALIDATION.md)
- [Client fixtures and lifecycle limits](client/README.md)
- [Event-log removal](validation/REMOVE-EVENT-LOG.md)
- [Initial import provenance](PROVENANCE.md)

For each change, record the commit, commands, results and unavailable checks.
Distinguish host checks, target compilation and native/QEMU execution. Historical
validation records are not evidence that a new commit has passed those checks.
