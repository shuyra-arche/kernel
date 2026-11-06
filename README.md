# Kernel

Shuyra's RV64 kernel, canonical ABI and typed client mechanisms.

## Layout

| Path | Responsibility |
| --- | --- |
| `api/` | Canonical ABI, shared types, codecs and macros; `kernel-api` re-exports Env and Wire |
| `client/` | Typed calls, trap transport and reusable memory, task and TLS mechanisms |
| `server/` | Kernel implementation, build script and linker script |
| `server/allocator/` | Physical allocation mechanisms |
| `server/sbi/` | SBI calls used by the kernel |
| `.github/workflows/` | Compilation checks and source version releases |

One root Cargo workspace and lockfile cover all packages. Client and Server
consume the same API; Server does not depend on Client. Runtime stays in
[Platform](https://github.com/shuyra-arche/platform). Cluster is an organizational
boundary, not a configuration file.

## Build

Install Rust through rustup; `rust-toolchain.toml` pins the toolchain and RV64
target. Run from the repository root:

```sh
cargo build --locked --workspace --target riscv64gc-unknown-none-elf
cargo build --locked --workspace --release --target riscv64gc-unknown-none-elf
cargo build --locked -p kernel --all-features --target riscv64gc-unknown-none-elf
```

The Server package is still named `kernel`; its executable is still `shuyra`.
Its manifest is now `server/Cargo.toml`. Build output remains under the root
`target/`. The optional `semihosting` feature exports crash reports. Normal
assertions, lock-order checks and crash diagnostics remain available.

Legacy suites, fixtures, test workspaces, boot self-tests and fault-injection
features have been removed. CI checks compilation only. Boot and system
integration validation belong to [Vanllia](https://github.com/shuyra-arche/vanllia),
which assembles the selected Units. Compilation alone does not establish guest
execution or resource reclamation.

## Versions

The repository has one source version in `[workspace.package].version`; every
package inherits it. API, Client and Server are released together at one Git
commit. A release records ABI/API compatibility changes and relevant validation.
Consumers should pin a release tag or exact revision, coordinating a single
canonical API source across their dependency graph.

To release, update the root version and regenerate `Cargo.lock`, merge through a
PR, then push a matching tag such as `0.0.1` on that merged commit. The release
workflow checks the version, main-branch ancestry and builds, then creates a
GitHub Release with generated change notes. Tags with a prerelease suffix create
prereleases. Release notes can be edited to explain compatibility changes.

Releases identify source versions. They upload no ELF, binary or image; GitHub
provides its normal source archives. A Cargo version declaration is not itself a
published release, and the workflow does not publish packages to crates.io.

## Source

Kernel and API derive from [anreonyr/sqware](https://github.com/anreonyr/sqware)
commit `672d8d2deafc8067c94b6af340a5f89c6a765c9c`. Client mechanisms derive from
[Execution](https://github.com/shuyra-arche/execution) commit
`1b333a28bc4c34beaea3f7986a9bb6e8ab2db414`. SBI was imported without source changes
from [SBI](https://github.com/shuyra-arche/sbi) commit
`2dc10bafb919eb2e8f6ca37c95cfd56f9a05831d`, originally extracted from
`anreonyr/sqware` at `c970afaf7dae1145633278253351be729b625717` (`crates/sbi`).

Original author identities and migration records remain in Git history. The
source checkpoints contained no license grant; this repository adds none.
Preserve third-party notices with their code.
