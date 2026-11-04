# Provenance

> **Historical upstream evidence.** This file records the original standalone
> extraction from `anreonyr/sqware` and is retained for provenance history only.
> For the current `kernel-std-work` import and canonical standalone source
> repositories, see [`../PROVENANCE.md`](../PROVENANCE.md).

## Source snapshot

- Repository: <https://github.com/anreonyr/sqware.git>
- Frozen commit: `66bb8b5c342c50b141eb935a85efdfcecb91451b`
- Commit date: `2026-10-26T21:14:19+08:00`
- Branch: `codex/shuyra-execution-extraction`
- Extracted subtree: `crates/execution`
- Source worktree state at extraction: clean; `git status --porcelain` was empty.
- Tracked files in the subtree: 36 (30 `.rs`, one of which is `build.rs`;
  `link.ld`; three `Cargo.toml`; two `.gitignore`).

The snapshot was copied one path at a time from the output of
`git ls-files crates/execution` at the frozen commit. No untracked file was
copied except the two owner lock files described below, and no `target/`
content was copied. The copy is not from an older on-disk provider and not
from a different branch.

## File mapping

For every row in [MIGRATION-MAP.md](MIGRATION-MAP.md), the source is
`crates/execution/<Path>` at `66bb8b5c` and the destination is `<Path>` in this
repository. Thirty-four of the 36 tracked files are byte-identical. The two
intentional adaptations are:

| File | Source SHA-256 | Destination SHA-256 | Change |
| --- | --- | --- | --- |
| `Cargo.toml` | `8d8f995df498a3b8cd83a52c86a5059647d1b10703480f9c79f8fdf6364637c5` | `12b12fa5f8c02abafe4a733b5b95e6028801386cff48dff68f27ea41cf6d0a2a` | Standalone workspace: explicit `version = "0.1.0"` and `edition = "2024"`, added `[workspace] resolver = "3"`. Package name, `links = "execution"`, all `[lib]` flags and all three dependency requirements are unchanged. |
| `tests/.gitignore` | `9cc2c8cc102e0b2a0a177c8f594abc9b9776300a04473c24b45d52c1007a4119` | `306fd52e74fca6746e12acc750f232de15e71da917756a1270d74c91f5eb7368` | Removed the `Cargo.lock` ignore so the owner lock can be tracked; kept `/target/`. |

`tests/unit/.gitignore` is byte-identical
(`306fd52e74fca6746e12acc750f232de15e71da917756a1270d74c91f5eb7368`) and
already contained only `/target/`.

All production Rust (`src/**/*.rs`), `build.rs`, `link.ld`, all six owner test
Rust files and both owner manifests are byte-for-byte unchanged. In particular,
`build.rs` still emits only:

```text
cargo::metadata=linker_script=<absolute path to link.ld>
cargo::rerun-if-changed=link.ld
```

`build.rs` was not modified to execute a link, add linker arguments, or embed a
host stub.

## Standalone manifest

The root `Cargo.toml` is the only adapted manifest. It retains:

- `name = "execution"` and `links = "execution"`;
- `[lib] path = "src/lib.rs"`, `test = false`, `bench = false`,
  `doctest = false`;
- `env` at `https://github.com/shuyra-arche/env.git`, rev
  `d3000253cd5d13d466e91fafd4dbb27a5ec5aaae`;
- `talc = "=5.1.1"` and `spinning_top = "=0.3.0"`, both
  `default-features = false`.

It replaces the source workspace inheritance with explicit
`version = "0.1.0"` and `edition = "2024"`, and adds only `[workspace]` with
`resolver = "3"`. There is no `[patch]`, `[replace]`, local path dependency,
old-source path link, or Git/cache projection. The source root
`.cargo/config.toml` (workspace target and QEMU runner) was not copied.

## Locks

All three locks are candidate tracked files. None is ignored.

- Root `Cargo.lock`: generated normally in this new workspace with an actual
  `cargo generate-lockfile --offline`. It has 15 package entries: 14 resolved
  packages plus the path package `execution`. SHA-256
  `34fd235f03bd0c4af6690cd1d033c87c3eef99f3936b76612db97329014d70f5`.
  It was not copied from the source workspace lock.
- `tests/Cargo.lock`: copied byte-identically from the source worktree, where
  it existed but was ignored by the original `tests/.gitignore`. 15 entries.
  SHA-256 `e411b5243a39fa6126d418c790709ac23385ea97a4b333d94240b0ee160292c8`.
- `tests/unit/Cargo.lock`: copied byte-identically from the source worktree.
  10 entries. SHA-256
  `a8160b6645ce154256132b327da4ead36f41a26adb32c91921d0eb9199709a3b`.

The two owner locks keep their already-verified versions, including
`env@d3000253`, `talc@5.1.1`, `spinning_top@0.3.0`, `erra@0.2.0` and
`unicode-ident@1.0.24`. The normally generated root lock resolves
`unicode-ident@1.0.27`; the three workspaces resolve independently and no lock
was rewritten to force a common indirect version.

The source workspace lock (934 lines, SHA-256
`db96275f12183fa02c9c4748e962287b0ae636414f4d397ef604019c3d449abb`)
was deliberately not copied. `cargo metadata --locked --offline` reports:

- root `execution 0.1.0` at this path;
- `env 0.1.0` from
  `git+https://github.com/shuyra-arche/env.git?rev=d3000253cd5d13d466e91fafd4dbb27a5ec5aaae#d3000253cd5d13d466e91fafd4dbb27a5ec5aaae`;
- `talc 5.1.1` and `spinning_top 0.3.0` from the crates.io registry;
- transitive `wire@0ff77dc4573fa2620d2fe995a18c01949d9c7e57` and
  `mold@f52501f4c91b686eded8b4bcd743b4e96084b7f9`, as pinned by `env`.

## Authorship history

The source subtree has 11 commits. Ten are authored by
`anreonyr <anreonyr@gmail.com>`; one is authored by
`Serial Device <anreonyr@gmail.com>`. Every one of the 36 current paths was
last touched by `anreonyr <anreonyr@gmail.com>`.

```text
66bb8b5c342c50b141eb935a85efdfcecb91451b 2026-10-26 anreonyr      refactor(execution): expose memory mechanisms for owned fixtures
e6302af2156a6fe3766d3ccc22f080a481948bf3 2026-10-26 anreonyr      Consume published env ABI and migrate its owned contract tests
2c17c3877e3f8c67d9db5634d693fa4303d47f8c 2026-10-26 anreonyr      Separate Unit declarations, runtime composition and image building
4c313f57d1b02add5024d67c0d6926381e1c1d3b 2026-10-16 anreonyr      feat(memory): authorize transfers and notify affected views
e53980793a86ddc7dba7ee9bec367fb15bcc35bc 2026-10-16 anreonyr      feat(memory): add residency-backed traversal and atomic unmapped transfer
75ef237326461a0a29dce1191240de82b623057b 2026-10-15 anreonyr      feat(memory): replace VA calls with source references and view identities
74cf41dc49b4ae74f7a5da91ce6562ba6007e4f1 2026-10-13 anreonyr      refactor(space): unify frames, mapping trees and Pole lifecycles
4f11e4436f4b85f267cc83b8224e3c13f789e0ac 2026-10-10 anreonyr      fmt
307a56df7b774d134aac4394d5dbe45d61f75b1e 2026-10-10 Serial Device refactor(unit): unify task and team state and redesign UnitCall
22e2bf2f9c563c71c5303d838318406e51f8d92e 2026-10-07 anreonyr      refactor: align execution and capability vocabulary
85804d869d9c89ec6e400c0107e35b47f28debc9 2026-10-07 anreonyr      refactor: replace runtime with execution and resource
```

The original history is preserved in the source repository at and before the
frozen commit. This snapshot does not rewrite, squash, or replay that history.
It had no `.git` directory when handed to the root reviewer; Git initialization
and the first commit are owned by the root.

## License

The source repository has no tracked `LICENSE`, `COPYING` or `NOTICE` file, no
`license` field in `Cargo.toml`, and no SPDX declaration in the extracted
subtree. This repository adds no license and no SPDX identifier. No license is
granted by this repository.

## Verification record

Raw logs, exit codes, lock hashes, `cargo metadata` output and per-file hashes
are under `evidence/pr12/provider/`. The required independent provider checks
were:

- x86_64 library build with `--locked`, then the same command with
  `--locked --offline`;
- RV64 library debug and release builds with `--locked`, then both with
  `--locked --offline`;
- 14 owner tests through `tests/Cargo.toml` and 1 through
  `tests/unit/Cargo.toml`, each with `--locked` and again with
  `--locked --offline`.

All exited 0 and all 15 tests passed in both runs. No complete ELF was
produced, so startup linking was not verified. The host tests exercise the
generic fixture and mocks, not `Native`, the real `env` memory calls, or the
task runtime. Host `env::ecall::trap` remains an existing `unimplemented!`.
