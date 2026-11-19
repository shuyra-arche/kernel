# Kernel tests

Test code follows the responsibility it verifies. Host suites mirror the real
production owner under `basis/`, `entry/`, and `metal/`; shared support belongs
under `support/` only when it has multiple actual consumers.

Host tests exercise real state machines, failure handling, permission and
resource invariants without pretending that mocks prove actual RV64 behavior.

```sh
cargo +nightly test --locked --workspace --target x86_64-unknown-linux-gnu
cargo +nightly test --locked --manifest-path server/tests/runner/Cargo.toml --target x86_64-unknown-linux-gnu
```

Target suites exercise current Kernel code:

- `target/basis/nest/completion/`: CPU mapping retirement, retained references,
  request replay, CPU fences and optional multi-Hart completion.
- `target/basis/flat/`: physical page ownership, allocation failure, transfer
  and cancellation.
- `target/metal/riscv/iommu/`: real MMIO/DMA mechanisms, separately selected.

Build the standalone Host runner and select a Kernel revision:

```sh
cargo +nightly build --locked --manifest-path server/tests/runner/Cargo.toml --target x86_64-unknown-linux-gnu
verify=server/tests/runner/target/x86_64-unknown-linux-gnu/debug/verify
"$verify" nest --kernel "$PWD" --output /tmp/nest --harts 1
"$verify" flat --kernel "$PWD" --output /tmp/flat --harts 1
```

Set `QEMU_RISCV64` and `QEMU_OPENSBI`; QEMU 10.0.x and OpenSBI DBCN are
required. Ordinary PR CI runs one Hart. Explicit multi-Hart Nest/Flat runs
are available via `--harts 4`; no four-Hart test is part of normal CI.

The runner retains source identities, reviewed patches, compiler/build logs,
QEMU output and cleanup receipts. Host compilation, target execution and
physical hardware qualification are distinct claims. No old-ABI fixture,
migration harness or general-purpose compatibility runner is maintained.
