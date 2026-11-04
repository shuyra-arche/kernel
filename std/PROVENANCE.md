# Provenance and import mapping

This prototype imports frozen source snapshots. The nested upstream
`README.md`, `PROVENANCE.md`, and `MIGRATION-MAP.md` files are retained as
historical evidence and are marked as such in their headers where imported.

Current `kernel-std-work` starting base:
`a9986c82127cf25183bf5c30e7d42e4a9376b734`.

Source repositories: the four frozen SHAs below belong to four standalone
repositories, not to one monolithic `sqware` origin. Nested older upstream
provenance files may trace earlier ancestry back to `anreonyr/sqware`, but the
coordinator-verified clone origins for these exact snapshot worktrees are:

- execution: <https://github.com/shuyra-arche/execution.git>
- resource: <https://github.com/shuyra-arche/resource.git>
- ipc: <https://github.com/shuyra-arche/ipc.git>
- mold: <https://github.com/shuyra-arche/mold.git>

## Source snapshots

| Snapshot | URL | Commit | `src` tree |
|---|---|---|---:|
| execution | <https://github.com/shuyra-arche/execution.git> | `1b333a28bc4c34beaea3f7986a9bb6e8ab2db414` | `0397bd3ef629cdb657cb5c208d840d8c0b5718f7` |
| resource | <https://github.com/shuyra-arche/resource.git> | `dbc96834f50f094b5c386e54a0a2ae5560dcd7f8` | `927d500bc0fa88889ade216f3cce88c9ae5709a7` |
| ipc | <https://github.com/shuyra-arche/ipc.git> | `29ad97dc08d9dfc4ea6578b1b77874261229b3b5` | `8477dcdc78834d4d964c0c45d65f13cce53c9e7e` |
| mold | <https://github.com/shuyra-arche/mold.git> | `f52501f4c91b686eded8b4bcd743b4e96084b7f9` | `a6f9803777d222a9826147bbac2f867e0f916dbc` |

`mold` was read-only evidence for this worker. No macros from it were
implemented or linked; concurrent adapter work owns that seam.

The independent source audit at
`/workspace/std-evidence/independent-source-audit.json` found 59 imported Rust
sources (execution 18 unchanged + 5 modified; resource 20 unchanged; ipc 16
unchanged), zero missing, and the moved link/start/args bytes intact. This
repo's tab-separated `PROVENANCE.tsv` records the same 59 imported Rust source
files with portable `std/...` current paths, source repo/commit, source-relative
path, status, current sha256, and source sha256.

## Import method

`resource/src` and `ipc/src` were copied byte-for-byte from the snapshots above.
`execution/src` was copied byte-for-byte, then only the files listed below were
modified. `execution/link.ld` and `execution/build.rs` were copied unchanged to
`std/rt/`; `execution/tests` and unit/resource/ipc host test harnesses were also
copied, then only their manifests and the execution general harness were adapted
to canonical path dependencies and the moved args test.

The complete per-file hash mapping is split. `PROVENANCE.tsv` (tab-separated,
with relative current paths and fixed source commit/relative source paths)
covers the 59 imported library Rust source files. The moved `link.ld`/`build.rs`
hashes are represented in the markdown table below and in the verifier, not in
the TSV. The TSV is not a blanket hash of every new manifest, test harness,
fixture, or doc file.

## Changed files

| Current file | Source file | Status | Current sha256 | Source sha256 |
|---|---|---|---:|---:|
| `std/execution/src/lib.rs` | `execution/src/lib.rs` | modified | `1f36daf41c8886c9958266dc1235e2f66ae2f36fe65c6ec450eb62c9dc8bf685` | `77dd91a6d1f415f6965b2e6aa6e329c5a8dfb85e284ca82993891d13a293e26e` |
| `std/execution/src/memory.rs` | `execution/src/memory.rs` | modified | `d22de0e12643cd29f024fa5fa77803101c6854ef6f1692a3fbbfe6158243bffb` | `9256ede3ce3f830c3f4f2068df17c398c97f4344b56146510c2fd020a6a70a38` |
| `std/execution/src/memory/allocator.rs` | `execution/src/memory/allocator.rs` | modified | `5cacfc2c941edc96281190c0938161fa4c4d5d319df05f1076f4178e08115809` | `2258198e7a081b18dc2c7791024bba070af073fe127f701004ceb7b9528b3f7f` |
| `std/rt/src/boot.rs` | `execution/src/boot.rs` | moved+modified | `814297372dc13194bb5c4f6b50af7d609b819544ef9ea8a245a0e3499f1503fd` | `c56a612dba82a7e1402748a8110de68988180363ef99c96bedf7caa11cbdaf2d` |
| `std/rt/src/boot/args.rs` | `execution/src/boot/args.rs` | moved unchanged | `7e925287630dbea81aa16478809bf5e402362aeb8ce8021e52a5661c01ac4e11` | `7e925287630dbea81aa16478809bf5e402362aeb8ce8021e52a5661c01ac4e11` |
| `std/rt/src/boot/panic.rs` | `execution/src/boot/panic.rs` | moved+modified | `c4a6267c22ed93961617f2710b4b1946dff7c85701c9540b74cc12c94c9ff54f` | `c31f93ac68185c95e7e36b9b8efb6148e111bac935975943862effe98610160e` |
| `std/rt/src/boot/start.rs` | `execution/src/boot/start.rs` | moved unchanged | `f1e4adaa7459e83e599634490e1318f4b1d0cd078bfe9a06a25c4c94fd844de8` | `f1e4adaa7459e83e599634490e1318f4b1d0cd078bfe9a06a25c4c94fd844de8` |
| `std/rt/link.ld` | `execution/link.ld` | moved unchanged | `e9ba425282fd6484497ef41eb9151da29c069f3c2c85d5d91a16d5cb72fcfe60` | `e9ba425282fd6484497ef41eb9151da29c069f3c2c85d5d91a16d5cb72fcfe60` |
| `std/rt/build.rs` | `execution/build.rs` | moved unchanged | `7083e99c4ea6bb7efb8488d07c449f0a45490eed68db7faa04250a81e4bb089c` | `7083e99c4ea6bb7efb8488d07c449f0a45490eed68db7faa04250a81e4bb089c` |

Deleted from the runtime-free execution copy: `std/execution/src/boot.rs`,
`std/execution/src/boot/args.rs`, `std/execution/src/boot/panic.rs`, and
`std/execution/src/boot/start.rs` (the latter two moved to rt as shown above).

## New files owned by this prototype

- `std/Cargo.toml`, `std/src/lib.rs`
- `std/execution/Cargo.toml`, `std/resource/Cargo.toml`, `std/ipc/Cargo.toml`
- `std/rt/Cargo.toml`, `std/rt/src/lib.rs`
- `std/fixtures/*`
- `std/tests/abi-identity/*`
- `std/rt/tests/args-host/*`
- `std/verify.sh`, `std/README.md`, `std/PROVENANCE.md`, `std/PROVENANCE.tsv`,
  `std/VALIDATION.md`

No ABI definitions were copied. The prototype uses canonical
`kernel-api-env`, `kernel-api-wire`, and `kernel-api-macros` by path.
