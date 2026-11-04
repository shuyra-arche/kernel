# Migration map

> **Historical upstream evidence.** This file records the original standalone
> extraction from `anreonyr/sqware` and is retained for provenance history only.
> For the current `kernel-std-work` import and canonical standalone source
> repositories, see [`../PROVENANCE.md`](../PROVENANCE.md).

Source: `crates/execution` at frozen commit
`66bb8b5c342c50b141eb935a85efdfcecb91451b`,
branch `codex/shuyra-execution-extraction`,
repository <https://github.com/anreonyr/sqware.git>.

The source prefix is `crates/execution/`; the destination prefix is this
repository root. Every tracked subtree file has exactly one destination. The
table lists all 36 tracked files and their source SHA-256 at the frozen commit.
For the 34 byte-identical files the destination hash is the same; the two
adapted destination hashes are recorded after the table.

## Tracked files

| Path | Source SHA-256 | State |
| --- | --- | --- |
| `Cargo.toml` | `8d8f995df498a3b8cd83a52c86a5059647d1b10703480f9c79f8fdf6364637c5` | adapted, see below |
| `build.rs` | `7083e99c4ea6bb7efb8488d07c449f0a45490eed68db7faa04250a81e4bb089c` | byte-identical |
| `link.ld` | `e9ba425282fd6484497ef41eb9151da29c069f3c2c85d5d91a16d5cb72fcfe60` | byte-identical |
| `src/boot.rs` | `c56a612dba82a7e1402748a8110de68988180363ef99c96bedf7caa11cbdaf2d` | byte-identical |
| `src/boot/args.rs` | `7e925287630dbea81aa16478809bf5e402362aeb8ce8021e52a5661c01ac4e11` | byte-identical |
| `src/boot/panic.rs` | `c31f93ac68185c95e7e36b9b8efb6148e111bac935975943862effe98610160e` | byte-identical |
| `src/boot/start.rs` | `f1e4adaa7459e83e599634490e1318f4b1d0cd078bfe9a06a25c4c94fd844de8` | byte-identical |
| `src/lib.rs` | `77dd91a6d1f415f6965b2e6aa6e329c5a8dfb85e284ca82993891d13a293e26e` | byte-identical |
| `src/lock.rs` | `8977888a7b24e97d3c434db08777904e2f9d7e375866b5db3766edc5a75d6bee` | byte-identical |
| `src/memory.rs` | `9256ede3ce3f830c3f4f2068df17c398c97f4344b56146510c2fd020a6a70a38` | byte-identical |
| `src/memory/allocator.rs` | `2258198e7a081b18dc2c7791024bba070af073fe127f701004ceb7b9528b3f7f` | byte-identical |
| `src/memory/allocator/pages.rs` | `bf64d436a3c9c005bc024b5c44f057bbb07042b79319b26c1e2ff3732a7a88bc` | byte-identical |
| `src/memory/allocator/region.rs` | `e6cea614d834ebba47a15e060a28c8f7a6ae8f27b3c349acfa775a0869ee6a67` | byte-identical |
| `src/memory/allocator/slot.rs` | `9d2976e78c0ecf21e86e2eb76f79e1a08bf7430a7429cc05672b8be90d84debf` | byte-identical |
| `src/memory/allocator/tests.rs` | `099b85687a239f5c27ee1126a69524ca10ce6256c157a3889104d6e1bf064350` | byte-identical |
| `src/memory/mapping_kernel.rs` | `a4a7547daf0550c9b1f067c7696dc6afd2c5bb3254553e2bfdc97fcd9dfb7469` | byte-identical |
| `src/memory/mechanism.rs` | `c4bc74cd9bfd13f95ef7e682b5237eea6b5d366617e8c0c0a9e1e9f1eef3ad0a` | byte-identical |
| `src/memory/native.rs` | `16e837fcdf543f732f0109e0767787bc245c2fa25524345270fb7ab8246ef65e` | byte-identical |
| `src/memory/reference.rs` | `43b6955fb650acb9f18d5c17cf36a90a97cc727c2f6e194db2dd7932ae36ec27` | byte-identical |
| `src/memory/reference_kernel.rs` | `34fa07d38e760acc0934128b6e5f49d17b6d89c5e7f3d7077681788bc2ea07db` | byte-identical |
| `src/memory/view.rs` | `901733848925d0af98fa293da3c1bc0c1edae7ab169d14b380780a8502516d63` | byte-identical |
| `src/room.rs` | `94db7f0b2a58f49fc11540484b006f6abc6841a3e46cf955c68f7865f40edd06` | byte-identical |
| `src/unit.rs` | `4f467e4412931832f507b12413a90602679beed0d0f5f66ed148b1622e54adf0` | byte-identical |
| `src/unit/task.rs` | `98c67610c56cefeb68d2e371d7427bbdc479e8534265ac60ef4ae049ba7754c1` | byte-identical |
| `src/unit/tls.rs` | `1944d5b7df0f1c0c06563fabc3f6add746438ab3ecd3d97fde46a43e90b05a5d` | byte-identical |
| `src/unit/tls_slot.rs` | `d7c4dbb33f2bb8e5c8d6b97a2f49634b7599778a33df598804d4a4798432af69` | byte-identical |
| `tests/.gitignore` | `9cc2c8cc102e0b2a0a177c8f594abc9b9776300a04473c24b45d52c1007a4119` | adapted, see below |
| `tests/Cargo.toml` | `3cfb5ffdff28e0dd57f06606ea8a7c035e260096432b0b33ac7607456d557ebb` | byte-identical |
| `tests/fixtures/kernel.rs` | `2cb05ab373dcd5b54aa3366435f06fd5d367e116dba182f0c10ecedccb980165` | byte-identical |
| `tests/fixtures/state.rs` | `245b28c280e4d0b7a1d861415005d3d567cabd951179c75e950ce84ff0e53946` | byte-identical |
| `tests/lib.rs` | `e600205dc20bbb5c196bd0f5dff6f4e6281659bb83149bab2e367a0c4ba8ee4d` | byte-identical |
| `tests/mechanism/mod.rs` | `12757b065ae2db9f13b4af418604d28b663d763f3cb2434852c621e3e2fa8d03` | byte-identical |
| `tests/mechanism/tests.rs` | `82af58fd1e25fdbf6a8516d1e71e3141740974411e1df56c181438cea0259f84` | byte-identical |
| `tests/unit/.gitignore` | `306fd52e74fca6746e12acc750f232de15e71da917756a1270d74c91f5eb7368` | byte-identical |
| `tests/unit/Cargo.toml` | `debf3b6074150cc3865b761ab0717142a3322ed616d85543e46a6ef194ac9b2d` | byte-identical |
| `tests/unit/lib.rs` | `a717ecd8a62e43d568222c5fb84ea052bcec225225389b98d23b39557105eac5` | byte-identical |

Thirty-four files are byte-identical. The two adapted files have these
destination hashes:

| Path | Destination SHA-256 | Exact change |
| --- | --- | --- |
| `Cargo.toml` | `12b12fa5f8c02abafe4a733b5b95e6028801386cff48dff68f27ea41cf6d0a2a` | `version.workspace = true` → `version = "0.1.0"`; `edition.workspace = true` → `edition = "2024"`; inserted `[workspace]` / `resolver = "3"`. |
| `tests/.gitignore` | `306fd52e74fca6746e12acc750f232de15e71da917756a1270d74c91f5eb7368` | Removed the `Cargo.lock` line; kept `/target/`. |

`tests/unit/.gitignore` was checked and remains byte-identical; it already
ignored only `/target/`, so its lock is trackable.

## Added provider infrastructure

These files are part of the snapshot but were not tracked at
`crates/execution/<Path>`:

| Path | Origin | SHA-256 | Purpose |
| --- | --- | --- | --- |
| `rust-toolchain.toml` | source root `rust-toolchain.toml`, byte-identical | `3d6cdf686f7dc362f1ce8af733e72339e3cb4ba8996dc9e31aef4682998e0fb6` | Pin nightly `2026-09-04` and the RV64 target. |
| `Cargo.lock` | generated in this workspace with `cargo generate-lockfile --offline` | `34fd235f03bd0c4af6690cd1d033c87c3eef99f3936b76612db97329014d70f5` | Minimal root lock; 14 resolved packages plus `execution`. |
| `tests/Cargo.lock` | source worktree, ignored but present; copied byte-identically | `e411b5243a39fa6126d418c790709ac23385ea97a4b333d94240b0ee160292c8` | Owner lock, versions preserved. |
| `tests/unit/Cargo.lock` | source worktree, ignored but present; copied byte-identically | `a8160b6645ce154256132b327da4ead36f41a26adb32c91921d0eb9199709a3b` | Unit owner lock, versions preserved. |
| `.gitignore` | new | `306fd52e74fca6746e12acc750f232de15e71da917756a1270d74c91f5eb7368` | Ignore only `/target/`; locks are not ignored. |
| `README.md` | new | recorded in `evidence/pr12/provider/snapshot.sha256` | Provider scope, build/test commands, limits. |
| `PROVENANCE.md` | new | recorded in `evidence/pr12/provider/snapshot.sha256` | Source identity, history, locks, license status. |
| `MIGRATION-MAP.md` | new | recorded in `evidence/pr12/provider/snapshot.sha256` | This one-to-one map. |

No source file from another crate, no consumer, and no older implementation
was copied. `target/` is outside the snapshot.

## Verification

The 14 tests in `tests/` are 5 allocator tests, 1 startup-argument test and 8
mechanism tests. `tests/unit/` has 1 Unit lifecycle test. Both owner suites were
run with explicit `--manifest-path`, `--target x86_64-unknown-linux-gnu` and
`--locked`, then repeated with `--locked --offline`. Both runs passed 14/14 and
1/1. The root `[lib] test = false` does not disable Cargo's auto-discovered
integration target `tests/lib.rs`; this snapshot leaves that auto-discovery and
the original test entry untouched.

See `PROVENANCE.md` for the exact lock resolution, authorship and license
status. Raw evidence is under `evidence/pr12/provider/`.
