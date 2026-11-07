# Kernel

Shuyra's RV64 kernel, public contracts and typed call transport.

## Layout

| Path | Responsibility |
| --- | --- |
| `api/` | One `kernel-api` package: semantic model, call ABI and boot contracts |
| `client/` | Typed calls and RV64 trap transport |
| `tools/abi-macros/` | Compile-time Kernel operation and ABI generation |
| `server/` | Kernel implementation, build script and linker script |
| `server/allocator/` | Physical allocation mechanisms |
| `server/sbi/` | SBI calls used by the kernel |
| `.github/workflows/` | Compilation checks and source version releases |

One root Cargo workspace and lockfile cover all packages. Client and Server
consume the same API; Server does not depend on Client. Language runtime/std support belongs to the separate Toolchain workstream.
Platform denotes hardware porting; Kernel defines no Runtime/Execution layer. Cluster is an organizational
boundary, not a configuration file.

## Control-flow boundaries

Kernel rebuild work is tracked in [#14](https://github.com/shuyra-arche/kernel/issues/14).
The model starts with data and control flows crossing isolation boundaries.
Under Server's `core/work`, `unit` owns execution control, lifecycle and results;
`sign` owns notification sources, wait registration and routing; `room` owns
scheduling and execution handoff. A suspended operation stays with its Task;
Join results and memory conditions are interpreted by their owning domains.
Team/Task pause and resume decisions use explicit state transitions before
applying scheduling effects under the commit boundary.

This first migration keeps the existing public calls, notification policies and
source keys. It does not yet implement the new Cell/View/Pend model or public
Sign objects and reliable-event delivery.

## Memory boundaries

`Flat` organizes backing slots, residency and frame transfer; `Nest::Mapping`
projects a Flat range into an address space. Mapping references retain frames
while hardware can still access them; they do not create independent physical
ownership. Permission changes compute the next logical ranges and PTE update
intents before Nest prepares and commits hardware effects. Pending invalidation
and retained resources remain explicit until shootdown succeeds.

The Server mapping types, source index, Space registry and wait keys use
`Mapping` to distinguish address projection from the planned cross-Unit `View`.
The current public ABI still names mapping handles `ViewId`, `ViewSelection` and
`ViewControl`; these are legacy mapping contracts, not the new interaction model.
Cell identity, unique ownership and Internal/External reclamation remain tracked
in #14; the existing Frame allocator is not yet a Cell implementation.

## Build

Kernel uses nightly Rust for `allocator_api`. The repository does not impose a
local toolchain override. CI installs the latest nightly without pinning a date.
To install or update nightly and build locally, run from the repository root:

```sh
rustup toolchain install nightly --profile minimal --target riscv64gc-unknown-none-elf
cargo +nightly build --locked --workspace --target riscv64gc-unknown-none-elf
cargo +nightly build --locked --workspace --release --target riscv64gc-unknown-none-elf
cargo +nightly build --locked -p kernel --all-features --target riscv64gc-unknown-none-elf
```

The Server package is still named `kernel`; its executable is still `shuyra`.
Its manifest is now `server/Cargo.toml`. Build output remains under the root
`target/`. The optional `semihosting` feature exports crash reports. Normal
assertions, lock-order checks and crash diagnostics remain available.

Legacy suites, fixtures, test workspaces, boot self-tests and fault-injection
features have been removed. CI builds RV64 and runs focused host call-boundary
and byte-layout regressions. Run the host checks explicitly (substitute your host
target on Windows). Use `cargo +nightly` in place of `cargo` below to select the
same channel as CI:

```sh
cargo test --locked -p kernel-api --tests --target x86_64-unknown-linux-gnu
cargo test --locked -p kernel-client --test call --target x86_64-unknown-linux-gnu
cargo test --locked -p kernel-api-macros --test declaration --target x86_64-unknown-linux-gnu
```

Tests, compile-fail fixtures, expected diagnostics and test support stay under
each owning package's `tests/` tree, grouped by ABI, boot, call or declaration
behavior. Suite entry points use `tests/<domain>/mod.rs` and explicit Cargo
`[[test]]` declarations. The production libraries remain unconditionally
`no_std`; host integration tests use `std`. `trybuild` is a test-only dependency for public
macro and operation type contracts. Library doctests are disabled.

Host call checks exercise the public `invoke_with` boundary with a recording
transport; they do not recompile private wrappers or perform native traps. Boot and system
integration validation belong to [Vanllia](https://github.com/shuyra-arche/vanllia),
which assembles the selected Units. Compilation alone does not establish guest
execution or resource reclamation.

Generic byte codecs and service-protocol macros are no longer part of Kernel.
The [API migration notes](api/README.md#source-migration-and-abi-compatibility) list removed
source exports and preserved ABI layouts. Kernel has no Weave dependency.

## Versions

The repository has one source version in `[workspace.package].version`; every
package inherits it. API, Client and Server are released together at one Git
commit. A release records ABI/API compatibility changes and relevant validation.
Consumers should pin a release tag or exact revision, coordinating a single
canonical API source across their dependency graph.

To release, update the root version and regenerate `Cargo.lock`, merge through a
PR, then push a matching `d<version>` tag such as `d0.0.1` on that merged commit. The release
workflow checks the version, main-branch ancestry and builds, then creates a
GitHub Release with generated change notes. Tags with a prerelease suffix create
prereleases. Release notes can be edited to explain compatibility changes.

Releases identify source versions. They upload no ELF, binary or image; GitHub
provides its normal source archives. A Cargo version declaration is not itself a
published release, and the workflow does not publish packages to crates.io.

## Source

Kernel and API derive from [anreonyr/sqware](https://github.com/anreonyr/sqware)
commit `672d8d2deafc8067c94b6af340a5f89c6a765c9c`. Historical Client mechanisms derive from
[Execution](https://github.com/shuyra-arche/execution) commit
`1b333a28bc4c34beaea3f7986a9bb6e8ab2db414`. SBI was imported without source changes
from [SBI](https://github.com/shuyra-arche/sbi) commit
`2dc10bafb919eb2e8f6ca37c95cfd56f9a05831d`, originally extracted from
`anreonyr/sqware` at `c970afaf7dae1145633278253351be729b625717` (`crates/sbi`).

Original author identities and migration records remain in Git history. The
source checkpoints contained no license grant; this repository adds none.
Preserve third-party notices with their code.

Room conditional waits (`WaitValue` / `WakeValue`) compare a resident, naturally
aligned atomic `u32` and enqueue under the same commit lock as wake. Queues are
private to an address space; wake removes one or all current waiters and never
stores a notification. Timeout, task death and space retirement use the existing
wait-ticket cleanup. Callers keep the object and mapping alive and recheck their
condition after a successful wait. Concurrent mapping transactions report `Busy`.
