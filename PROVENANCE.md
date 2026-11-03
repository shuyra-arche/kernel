# Provenance

## Source

- Repository: `https://github.com/anreonyr/sqware.git`
- Public draft PR: 45
- Commit: `672d8d2deafc8067c94b6af340a5f89c6a765c9c`
- Branch at snapshot: `codex/shuyra-device-contracts-extraction`
- Snapshot method: `git archive` of the 490 tracked `kernel/` paths at that
  commit, extracted so the former `kernel/` directory is this repository root.
  Four additional ignored test locks were prepared offline in the Source
  worktree and copied separately, as listed below.
- Source working tree was clean at the snapshot commit.

The snapshot contains 490 tracked Source files. `SOURCE-MANIFEST.tsv` maps
every one to its provider path and both SHA-256 values: 489 are byte-identical,
and only `Cargo.toml` is marked `adapted-manifest`.

## Root manifest adaptation

`kernel/Cargo.toml` was adapted only to add the standalone workspace facts:

- `[workspace] members = [".", "allocator", "tests/api"]`, resolver 3;
- `[workspace.package] version = "0.1.0"`, edition 2024;
- `[profile.dev] debug = true`, `panic = "abort"`;
- empty `[profile.release]`.

No feature, dependency, lint, bin, test, package, or profile value changed.
No production Rust, linker script, build script, health test, include, or
fixture semantics changed.

## Authors recorded by Source history

Git history for the selected kernel paths records:

- `Serial Device <anreonyr@gmail.com>`
- `anreonyr <88045926+anreonyr@users.noreply.github.com>`
- `anreonyr <anreonyr@gmail.com>`
- `anreonyr <anreonyr@local>`

## License

No `LICENSE`, `COPYING`, or `NOTICE` file is tracked at the Source commit, and
the selected Source manifests declare no `license` or `authors`. This snapshot
adds no license and asserts no copyright permission. Publication does not imply
a license.

## Provider additions and test lock provenance

| File | Purpose |
| --- | --- |
| `Cargo.lock` | Cargo-generated minimal owned graph; not copied from Source root |
| `.cargo/config.toml` | RV64 target and the identical three rustflags; no runner/image alias |
| `.gitignore` | ignores build/trace output; root rule allows owned test locks |
| `rust-toolchain.toml` | copied byte-identically from Source |
| `README.md` | provider layout, commands, public test-tool dependency |
| `PROVENANCE.md` | this record |
| `SOURCE-MANIFEST.tsv` | 490-path source/provider hash map |
| `tests/{console-line,custody,frame-runs,trace}/Cargo.lock` | Root-prepared offline locks, copied from the prepared Source worktree |
| `tests/{dma-confirmation,export,pend,scheduler-dispatch,wait-block,weak-copies}/Cargo.lock` | tracked Source locks, copied byte-identically |

The four Root-prepared locks are force-added because the preserved nested
Source test `.gitignore` files still contain `/Cargo.lock`. Those `.gitignore`
files remain byte-identical to Source, as `SOURCE-MANIFEST.tsv` records.

## External test tool

Native health uses the public Integration checkout at the same full Source
commit, under `evidence/pr23/harness-source`. Its `SBI.bin` SHA-256 is
`c53d58aa9ab17f95fa1af159f0bf05fad5c5622ebc0f1084463435fa6e5752b6`.
The harness is an executable external test tool. It is not a Cargo dependency,
not copied into this repository, and no harness implementation is linked or
compiled here.

## Status

At the worker preparation checkpoint, this was an unpublished, uncommitted
candidate. Root owns acceptance and publication. Source consumer cutover and
deletion of the old implementation are separate later steps; publishing the
provider snapshot does not complete the Kernel extraction or stages 3–4.
