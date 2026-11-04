# Provenance

> **Historical upstream evidence.** This file records the original standalone
> extraction from `anreonyr/sqware` and is retained for provenance history only.
> For the current `kernel-std-work` import and canonical standalone source
> repositories, see [`../PROVENANCE.md`](../PROVENANCE.md).

## Source snapshot

- Repository: <https://github.com/anreonyr/sqware>
- Commit: `667f9635f21b2291563f06aaaf1a64cebd894aea`
- Branch at extraction: `codex/shuyra-resource-extraction`
- Pull request: PR #31. This is the extraction handoff identifier; the public
  PR page and a public clone were not fetched or checked in this task.
- Extracted path: `crates/resource`
- Snapshot method: `git archive` of the commit above, not the dirty worktree
  and not an older on-disk provider copy. The source worktree remained at
  `HEAD = 667f9635f21b2291563f06aaaf1a64cebd894aea` and clean.

The source commit has 26 tracked files under `crates/resource`: 20 Rust files
under `src/`, one `Cargo.toml`, and five files under `tests/capability/`
(`.gitignore`, `Cargo.toml`, `lib.rs`, `native.rs`, `tests.rs`). The capability
`Cargo.lock` was not tracked at that commit; it was ignored by the source
`.gitignore`. The ignored worktree file existed, but this provider's capability
lock was generated from the fixed manifest rather than claimed as a
source-tracked artifact.

## File mapping

| Upstream at `667f963` | Here | State |
| --- | --- | --- |
| `crates/resource/src/**/*.rs` (20 files) | `src/**/*.rs` | byte-identical |
| `crates/resource/Cargo.toml` | `Cargo.toml` | standalone rewrite, see below |
| `crates/resource/tests/capability/Cargo.toml` | `tests/capability/Cargo.toml` | byte-identical |
| `crates/resource/tests/capability/lib.rs` | `tests/capability/lib.rs` | byte-identical |
| `crates/resource/tests/capability/native.rs` | `tests/capability/native.rs` | byte-identical |
| `crates/resource/tests/capability/tests.rs` | `tests/capability/tests.rs` | byte-identical |
| `crates/resource/tests/capability/.gitignore` | `tests/capability/.gitignore` | removed `Cargo.lock`, kept `/target/` |
| source-root `rust-toolchain.toml` | `rust-toolchain.toml` | byte-identical |
| source-root `Cargo.lock` | `Cargo.lock` | copied then pruned, see below |
| none | `tests/capability/Cargo.lock` | generated, see below |
| none | root `.gitignore`, `README.md`, `PROVENANCE.md` | new provider metadata |

All 20 `src/*.rs` files and all three owned Rust files (`lib.rs`, `native.rs`,
`tests.rs`) are byte-for-byte identical to the source commit. The owned
`Cargo.toml` is also byte-for-byte identical. The test projection still uses
`#[path = "../../src/..."]` for the same original source modules and retains
the original fake Env; no business implementation was copied into another
test.

The source root's `Cargo.toml`, `.cargo/config.toml`, and programs
composition are not copied. The source-root `Cargo.lock` is used only as the
prune seed described below; it is not adopted unchanged.

The standalone root `Cargo.toml` keeps the original package name, exact `env`
Git revision, and `erra 0.2.0` with `default-features = false`. It replaces the
inherited workspace `version` and `edition` with explicit `version = "0.1.0"`
and `edition = "2024"`, preserves `[lib] test = false`, `bench = false`, and
`doctest = false`, and adds only an independent `[workspace]` with
`resolver = "3"`. It contains no `[patch]`, `[replace]`, local Git dependency,
or local cache projection. `tests/capability` keeps its own `[workspace]` and
its original `abi = { package = "env", ... }` and `erra` requirements.

## Dependency resolution

The root package's only direct dependencies are:

- `env` at `https://github.com/shuyra-arche/env.git`, rev
  `d3000253cd5d13d466e91fafd4dbb27a5ec5aaae`;
- `erra` `0.2.0` from the crates.io registry with default features disabled.

The resolved provider revisions reported by actual `cargo metadata` are:

- `env` `0.1.0` —
  `d3000253cd5d13d466e91fafd4dbb27a5ec5aaae`
- `wire` `0.1.0` (transitive through `env`) —
  `0ff77dc4573fa2620d2fe995a18c01949d9c7e57`
- `mold` `0.1.0` (transitive through `env`) —
  `f52501f4c91b686eded8b4bcd743b4e96084b7f9`

No provider revision or registry version was changed for this extraction.

## Lock infrastructure

The source-root `Cargo.lock` (tracked at `667f963`, SHA-256
`6256553b2a60f55473804b11c6401828d0895ad7381535ec71d22f29308165c4`,
100 packages) was copied as the starting point for the standalone graph. An
actual `cargo metadata --format-version 1` run then pruned it to the 10
packages still reachable from the standalone `resource` package:

```text
resource 0.1.0 (path)
env 0.1.0 (git d3000253)
wire 0.1.0 (git 0ff77dc4)
mold 0.1.0 (git f52501f4)
erra 0.2.0 (registry)
bitflags 2.13.1 (registry)
proc-macro2 1.0.107 (registry)
quote 1.0.47 (registry)
syn 2.0.119 (registry)
unicode-ident 1.0.24 (registry)
```

Those registry versions are the versions already present in the source-root
lock; the prune did not upgrade them. The resulting root `Cargo.lock` has
SHA-256 `76594a719cd7090d3504a7154eb4ce03e5e2d186097213207e40ac960eb62ba4`.

The capability workspace's independent `Cargo.lock` was seeded from the same
source-root lock and generated by an actual `cargo metadata --format-version 1`
run against its fixed, unchanged manifest. It has the same 10-package
resolution apart from the workspace package name
(`resource-capability-tests`) and SHA-256
`986dd3b8df67ac0766dfe6c0c61ec8985ced1263aa384c2ab4b691a3c1a363dd`. The source
never tracked a capability lock: this is generated candidate infrastructure,
not a copy of a Git-tracked `crates/resource/tests/capability/Cargo.lock`.

The owned `.gitignore` now ignores only `/target/`, so the generated capability
lock can be tracked. The root `.gitignore` ignores only `/target`. No lock file
is ignored.

## Verification records

The provider evidence is under `evidence/pr9/provider/`. It records raw cargo
exit codes for the locked RISC-V debug and release builds, the host check, the
22-test capability run before and after an actual `--offline` rerun, and the
externally seeded ABI probe. The capability lock hash was identical before the
first run, after it, and after the offline rerun. Root `cargo test` has no
owned assertions and is not reported as capability coverage. No QEMU run and
no consumer migration were performed.

## Attribution

The source commit author and committer is:

```text
anreonyr <anreonyr@gmail.com>
```

Unique authors in the real history of `crates/resource` include:

```text
anreonyr <88045926+anreonyr@users.noreply.github.com>
anreonyr <anreonyr@gmail.com>
anreonyr <anreonyr@users.noreply.github.com>
```

The wider source repository history also contains:

```text
Serial Device <anreonyr@gmail.com>
anreonyr <anreonyr@local>
```

The original history remains at the source URL above. This snapshot does not
rewrite or synthesize commit history.

## Licensing

The source project declares no license. At commit
`667f9635f21b2291563f06aaaf1a64cebd894aea`, no tracked `LICENSE`, `COPYING`, or
`NOTICE` file matched the project. This extraction adds no license text or
license metadata; do not assume a license that the source does not grant.
