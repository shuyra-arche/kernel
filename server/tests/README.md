# Kernel verification

Use the [root README](../../README.md#build-and-test) for host tests and RV64
compilation. Host tests cover ABI, state transitions and scripted failure
paths. The QEMU suite executes real RV64 traps, boot, scheduling, and CPU and
device translation on emulated hardware. Physical controller behavior needs
separate hardware evidence.

## Set up the QEMU fixture

Start with the root README's Rust toolchain. Install Python 3, GNU `nm` and
`sha256sum`, and an RV64 QEMU with `virt,iommu-sys` and EDU support. The IOMMU
harness records QEMU 10.0.13 as a validated version. Kernel boot tests use
OpenSBI, and the diagnostics test needs SBI DBCN. Set `QEMU_RISCV64` to select
a binary other than `qemu-system-riscv64`, or `QEMU_OPENSBI` to select a firmware
image instead of QEMU's default.

Check out Vanllia in a dedicated fixture checkout at the revision listed in
the `Fetch external QEMU fixture` step of the
[Check workflow](../../.github/workflows/check.yml). The setup below patches
the two fixture manifests to use the Kernel checkout under test and refreshes
their lockfiles. Run it once from the Kernel repository root with the fixture's
absolute path:

```sh
vanllia_fixture=/absolute/path/to/vanllia
kernel_dir=$(pwd -P)
export RUSTUP_TOOLCHAIN=nightly
export KERNEL_BOOT_MANIFEST="$vanllia_fixture/platform/riscv/Cargo.toml"
export KERNEL_PLATFORM_MANIFEST="$vanllia_fixture/tests/platform/qemu/Cargo.toml"

for manifest in "$KERNEL_BOOT_MANIFEST" "$KERNEL_PLATFORM_MANIFEST"; do
    cat >> "$manifest" <<EOF_PATCH

[patch."https://github.com/shuyra-arche/kernel.git"]
kernel = { path = "$kernel_dir/server" }
kernel-api = { path = "$kernel_dir/api" }
EOF_PATCH
    cargo metadata --manifest-path "$manifest" --format-version 1 >/dev/null
done
```

## Run the guest tests

The Cell guest consumes ABI v10. Its bootstrap CPU authority comes from the
ledger's `Name::Space` entry; `Name::Nest` entries describe external topology.
Ordinary Tasks start with no grants. Fixtures explicitly delegate creation,
Whole-Nest reservation or mapping-scoped authority before resuming a worker.
Reservations use `Nest.Reserve` without a source, then `Cell.Insert` and
`Cell.Publish`. A `MappingId` is read from `PendInfo.limits.scope`, never used as
a capability. `Nest.Release` closes a complete mapping reservation; returning a
child grant withdraws only that branch's publications.

Run the full ignored hardware suite in the same shell:

```sh
cargo +nightly test --locked -p kernel-memory-tests --test hardware \
    --target x86_64-unknown-linux-gnu -- --ignored --nocapture
```

To reproduce one CI group, use the [shard runner](hardware/shard.py). For
example, run the I/O group with `python3 server/tests/hardware/shard.py io`.
The runner's `--help` lists the groups. The large-Cell case in the memory group
requires 3 GiB of contiguous guest RAM.

## Read the evidence

Generated evidence lives under each guest's `target/evidence/` directory:

| Guest | What it exercises |
| --- | --- |
| [Cell](guest/cell/) | Kernel calls, scoped Nest authority, source-free reservations, unrelated per-position sources, sticky read-only copies, original-source Task retirement, CPU and IOMMU mappings, interrupts, scheduling, and task state. Includes a separate-Team, window-limited mapping delegate; Flat Block/Allow and holder transfer; read-only memory snapshot/Sign with real level-1 allocation. |
| [IOMMU](guest/iommu/) | The hardware backend directly, including queue completion and denied DMA after reuse. |
| [Diagnostics](guest/diagnostics/) | Panic, illegal instruction, pre-allocator failure and recursive reporting. |

The Cell cases require real read/write-fault probes after source withdrawal or
read-only delegation, and retain delayed-DMA and exact-fence checks. The
write-to-execute update case checks new instruction results without a guest
`FENCE.I`; QEMU execution is not proof of physical instruction-cache coherence.

CI uploads each group's evidence as a `guest-evidence-*` artifact. The
[latency](latency/README.md) and [real-time](realtime/README.md) notes describe
their focused scenarios and how to interpret their measurements.

To symbolize a diagnostic serial log, use the exact ELF recorded by the run.
From the repository root:

```sh
python3 tools/diagnostics/symbolize.py \
    server/tests/guest/diagnostics/target/evidence/panic-debug.elf \
    server/tests/guest/diagnostics/target/evidence/panic-debug-smp1-serial.log
```

Match the ELF to its recorded SHA-256. A stack-scan hit is a candidate address.
The report labels frame-chain evidence separately.

## Staged memory v11 host evidence

The `invocation` target imports the production next-memory descriptor dispatcher
and result receipt code. Task/user buffers and resource effects are fixtures;
it checks complete input copying, lost replies, replay, result inspection,
Pending/Failed retention and release of ledger/journal/COMMIT locks across an
effect boundary. It is not a native trap, permission or MMU test.

The `memory` target additionally imports production Flat/Page/Memo state and the
Page change registry. Its maintenance tests exercise exact Held-reference
consumption and refusal with strong/weak observers, overlapping blocks, stale
versions or unfinished hardware obligations. Memo observation tests retain the
exact Page and access across failed fences and whole-namespace withdrawal.
Receipt completion is explicitly scripted. See the [M3 evidence matrix](../../api/memory-m3.md#verification-and-limits)
for the boundary between these tests, ledger/lineage tests and RV64 compilation.
