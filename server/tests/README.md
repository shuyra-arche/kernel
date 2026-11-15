# Kernel verification

Host tests mirror their source owners under `basis/`, `entry/`, and `metal/`.
`Cargo.toml` declares independent nested `mod.rs` entries: mocks, imported private
source and target assumptions remain isolated. The original 20 harness boundaries
are preserved; the activity epoch contract is additionally split from RV64 PTE
storage into its actual `basis/nest/memo/activity` owner. A contract crossing
Cell, Flat and Nest stays at their nearest source parent, `basis/mod.rs`.

| Cargo host target | Entry / protected responsibility |
| --- | --- |
| memory | `basis/mod.rs`: Page/Flat/Nest ownership, retention, recovery and topology |
| translation | `metal/riscv/mmu/mod.rs`: real PTE storage, atomic A/D capture, decoding |
| activity | `basis/nest/memo/activity/mod.rs`: retained capture epochs and exhaustion |
| fault | `basis/nest/fault/mod.rs`: bounded nonconsuming event queues |
| iommu | `metal/riscv/iommu/mod.rs`: completion, failure, source and lifetime models |
| iommu-target (formerly hardware) | `metal/riscv/iommu/execution/mod.rs`: real QEMU MMIO/DMA runner; ignored by default |
| sign | `basis/sign/mod.rs`: endpoint state and wait admission |
| pend | `basis/pend/mod.rs`: authority, narrowing, creation and tracking |
| address | `basis/nest/address/mod.rs`: occupancy, placement, overflow and generations |
| name | `basis/name/mod.rs`: identity allocation, arenas, registries and exhaustion |
| unit | `basis/unit/mod.rs`: execution, membership and retirement |
| room | `basis/room/mod.rs`: deadlines, scheduling, timers, shutdown and reservations |
| sync | `metal/riscv/sync/mod.rs`: production admission, FIFO, diagnostics and UI lifetimes |
| allocator | `metal/allocator/mod.rs`: frame allocation and failure |
| registration | `basis/pend/ledger/mod.rs`: ledger, branch and export lifetimes |
| invocation | `entry/call/mod.rs`: production request delivery and replay |
| context | `metal/riscv/switcher/context/mod.rs`: extension context policy |
| console | `metal/riscv/console/mod.rs`: output admission |
| interrupt | `basis/room/interrupt/mod.rs`: receiver state and progress |
| latency | `metal/measure/mod.rs`: bounded distributions and whole-interval accounting |
| diagnostics | `entry/report/mod.rs`: frame evidence, emergency output and lifecycle ring |

Shared fixtures remain under `support/`. Each suite's synthetic crate root is
local to that harness. The opt-in `ticket-storage` bench remains under
`basis/name/cost.rs`; it is exploration, not a default test.

## Host and target compilation

Use the root [build commands](../../README.md#build-and-test). The runner is a
separate host-only workspace and is excluded from the RV64 workspace build:

```sh
cargo +nightly test --locked --workspace --target x86_64-unknown-linux-gnu
cargo +nightly test --locked --manifest-path server/tests/runner/Cargo.toml --target x86_64-unknown-linux-gnu
bash tools/tests/layout.sh
bash tools/tests/release.sh
bash tools/check-layout.sh
bash tools/check-tooling.sh
```

Host assertions do not execute RV64 traps or prove hardware quiescence.
`tools/check-layout.sh` scans all Git-tracked paths, including nested guests;
directory entries use `mod.rs`, with no same-stem file/directory pairs. This is
the explicit convention requested in #199/#201; shared policy PR #14 is pending.

## Native execution

Use nightly Rust, QEMU 10.0.x, GNU build tools and an identified OpenSBI image
with DBCN. Set `QEMU_RISCV64` and `QEMU_OPENSBI`. Clone Vanllia at the exact
revision in [runner/fixture](runner/fixture); the same commit supplies the shared
execution library. Vanllia owns native memory and System/Twine compositions.
Kernel owns its private completion, A/D, Flat and Room probes.

```sh
cargo +nightly build --locked --manifest-path server/tests/runner/Cargo.toml --target x86_64-unknown-linux-gnu
verify=server/tests/runner/target/x86_64-unknown-linux-gnu/debug/verify
"$verify" smp --kernel "$PWD" --fixture /path/to/vanllia --output /new/smp --profile release
"$verify" activity --kernel "$PWD" --fixture /path/to/vanllia --output /new/activity --profile release
"$verify" flat --kernel "$PWD" --fixture /path/to/vanllia --output /new/flat
"$verify" room --kernel "$PWD" --fixture /path/to/vanllia --output /new/irq --mode irq --events 16
```

Run both debug and release. SMP defaults to 1/2/4 Harts; A/D and Flat default
to 1/4. Activity also has separate `--unsupported` and `--stopped` scenarios.
Room correctness uses `--mode irq --events 16` on 1/4 and `--mode realtime
--harts 4`. Scheduling comparisons stay opt-in, using `--mode room --baseline
/path/to/control --rounds 5`; cold/warm study uses `--compare-room-warmup`.
Invalid counts, duplicate Harts and malformed baseline SHAs fail admission.

```sh
cargo +nightly run --locked --manifest-path /path/to/vanllia/tests/runner/Cargo.toml --target x86_64-unknown-linux-gnu -- native --kernel "$PWD" --fixture /path/to/vanllia --output /new/native-debug
cargo +nightly run --locked --manifest-path /path/to/vanllia/tests/runner/Cargo.toml --target x86_64-unknown-linux-gnu -- native --kernel "$PWD" --fixture /path/to/vanllia --output /new/native-release --profile release
cargo +nightly run --locked --manifest-path /path/to/vanllia/tests/runner/Cargo.toml --target x86_64-unknown-linux-gnu -- native --kernel "$PWD" --fixture /path/to/vanllia --output /new/reject --capsule-version 10
```

The last command is a negative test of the current boot contract, not an ABI 10
guest. System consumer verification uses Vanllia's `verify system` with the
pinned System and Twine checkouts in [Check](../../.github/workflows/check.yml).
Independent IOMMU backend execution remains available with
`cargo +nightly test --locked -p kernel-memory-tests --test iommu-target
--target x86_64-unknown-linux-gnu -- --ignored --nocapture`.

## Composition and evidence

[runner/](runner/) contains typed scenario selection and assertions. Its
[composition patches](runner/composition/) are reviewed test inputs. Each touched
production or fixture file must match its recorded preimage SHA-256 before
`git apply --check` and application in a disposable archive. Changed production
preimages require review; a baseline with incompatible source fails explicitly.
No runtime string replacement, production test feature, compatibility ABI,
permanent topology manifest or universal orchestration language is added.
Guest constants use typed Cargo environment inputs; guest/probe bodies stay
under `target/basis` or `target/metal`, never in production `src/`.

Outputs must be new directories. Each run records committed provider/fixture
revisions, exact compiler/Cargo/emulator/firmware identities, runner/suite/patch
hashes, original and resolved locks, both build commands, ELF/Capsule hashes,
logs and per-boot results. Provider substitutions resolve once, then build again
with `--locked`; neither input checkout is edited. Comparisons build both images
before alternating their boot order. Cold/warm boots share one platform ELF.
Every boot requires complete scenario assertions and a successful process exit;
a timeout, signal, nonzero exit or failure marker cannot pass. Evidence survives
failed builds and boots. Raw results belong in CI artifacts, not source control.

Ordinary Host-only changes run the inexpensive checks. Production/API/runner
changes select all affected target scenarios conservatively; a specific guest
change selects that scenario. Main/release runs execute all scenarios. Longer
comparisons require `Room comparison: requested` and exactly one
`Room baseline: <40 lowercase hex characters>` line in the PR body.

## Retired historical checks

At the user's explicit request, the ABI 10 Cell and diagnostics guests, their
legacy Kernel/platform harness, routing/shard tests and old real-time guide
are deleted. This also removes the pinned legacy hardware-fixture dependency.
The removed Cell cases covered scoped Nest grants, read/write faults, source
withdrawal, instruction updates, vectors/migration, delayed DMA, large Cells and
legacy scheduling. They are not claimed as equivalent current ABI 11 coverage.
Their source and past results remain in Git history. The native cases above
retain their own current contract; physical controller, vector/context hardware
and complete System acceptance remain separate evidence boundaries.

## Diagnostics

Use the exact ELF identified by an execution's SHA-256:

```sh
"$verify" symbolize /path/to/platform.elf /path/to/serial.log
"$verify" inventory --kernel "$PWD"
```

Symbolization selects Trap/frame evidence and passes literal PCs to
`llvm-addr2line`, preserving compressed-instruction return addresses. Stack-scan
hits remain candidate addresses; frame-chain evidence is labelled separately.
The lock inventory is textual navigation, not execution or lock-safety proof.
