# Kernel verification

Use the [root README](../../README.md#build-and-test) for host tests and RV64
compilation. Host tests cover ABI, state transitions and scripted failure
paths. The QEMU suite executes real RV64 traps, boot, scheduling, and CPU and
device translation on emulated hardware. Physical controller behavior needs
separate hardware evidence.

## Current native ABI 11 composition

The `Guest tests (QEMU)` check uses the pinned Vanllia native composition in
[Check](../../.github/workflows/check.yml). It patches API, Client and Server
to the current checkout, without an ABI feature. The historical fixture's
`memory-v11` dependency entries are removed before resolving the local provider.

With that fixture prepared, run from the Kernel root:

```sh
python3 /absolute/path/to/vanllia/tests/memory/run.py --kernel "$PWD"
python3 /absolute/path/to/vanllia/tests/memory/run.py --kernel "$PWD" --no-build --capsule-version 10
python3 /absolute/path/to/vanllia/tests/memory/run.py --kernel "$PWD" --profile release
```

Supply `QEMU_OPENSBI` and the reviewed QEMU 10.0.x executable. The native guest
covers Task/Team isolation, authority, faults, shared memory, maintenance,
withdrawal, lifecycle and virtio-blk/IOMMU shutdown. Evidence is uploaded as
`native-memory`. QEMU evidence does not establish physical hardware behavior
or native SMP acceptance. The historical scheduling/vector/diagnostics cases
below have not all been migrated to this native composition; their previous
results must not be reported as current ABI 11 acceptance.

## Historical ABI 10 guest suite

The remaining instructions describe the preserved ABI 10 suite. Run these only
from an ABI 10 Kernel checkout (for example Preview Alpha), with Vanllia fixture
`eb37b0b0db299513274864cec23cee509a3cd3cc`. They are not current ABI 11 CI commands.

## Set up the QEMU fixture

Start with the root README's Rust toolchain. Install Python 3, GNU `nm` and
`sha256sum`, and an RV64 QEMU with `virt,iommu-sys` and EDU support. The IOMMU
harness records QEMU 10.0.13 as a validated version. Kernel boot tests use
OpenSBI, and the diagnostics test needs SBI DBCN. Set `QEMU_RISCV64` to select
a binary other than `qemu-system-riscv64`, or `QEMU_OPENSBI` to select a firmware
image instead of QEMU's default.

Check out Vanllia in a dedicated fixture checkout at the revision listed in
the historical fixture pin above. The setup below patches
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

## Current memory ABI verification

Host tests check actual Flat/Page/Nest state transitions, failure
injection, authority and result-journal correlation. Host fixtures
do not execute guest traps or establish hardware quiescence.

The `translation` host target compiles the production RV64 PTE storage,
decoder and flags. Its atomic hardware participant checks preservation of
concurrent A/D evidence during permission changes and coherent full-word
snapshots. It does not enable A/D clearing or qualify a sampling backend.

The current native composition exercises real Task/Team startup, CPU mappings
and qualified IOMMU/Virtqueue paths using the single ABI 11 contract.
A successful QEMU device scenario is not a real-hardware proof.

Report the exact source/fixture revision, command, ABI selection,
firmware and emulator versions, output, errors and skipped checks
when publishing verification evidence.
