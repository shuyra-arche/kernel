# Kernel-owned published ABI13 CPU completion and recovery Guest

This standalone test Package owns the exact **executed** CPU-only ABI13 source
contracts previously composed from the historical Vanllia memory Guest.
It is not a product test or a new Kernel ABI. The selected Kernel provider
is the last compatible ABI13 revision with all four target mechanisms:
`b1062f60bfe96e625ed140952d9662319c70087e`.
The older `789ffd19...` source predates the retirement/finalization owners
and is not the baseline for the complete four-mode suite.

## Source and scope

The executable User Guest originated from Vanllia
`f2dfa14f5b1f7bd47ee84841ba5c7112e8475950`. The previously reviewed `user-fixture` and `smp-fixture`
transformations are incorporated into this test-only snapshot, with their
original exact preimage contracts retained in Git history and review.
Unused legacy authority, IPC and virtqueue scenario modules were **not**
copied because the original SMP composition did not execute them.

The unchanged active acceptance modules still exercise real Task permission,
Nest/Flat ownership and lineage, fault continuations, maintenance, memory
release/reuse and passive Save/Restore. No old PASS string replaces a native
assertion. Actual CPU completion, instruction fences, real mapping retirement, failure
replay and migration checks use the Kernel-owned frozen
[`smp.rs`](smp.rs) and [`recovery.rs`](recovery.rs), taken from the last
ABI13 Kernel revision. The real-Hart
[`probe.rs`](../metal/riscv/mmu/completion/probe.rs) and
[`instructions.rs`](../metal/riscv/mmu/completion/instructions.rs) retain
their existing Kernel ownership and exact byte identity across the ABI13
source revision.
The unchanged probe and instructions helpers are shared with their existing
Kernel owner; they are not copied into the Guest.
It also imports Kernel's existing
[`Stack`](../basis/room/pinned/stack.rs) support, preserving real memory
ownership and Task exit semantics.

The QEMU Platform is **CPU-only**, with no device, IOMMU or external route.
The Platform admits only paired QEMU virt with exactly 128 MiB coherent
RAM and 1/2/4 emulated Harts. Verified here: all nine scenario variants
on **one** Hart. No completed multi-Hart qualification or physical hardware
safety is claimed. Production Platform remains the immutable Git-pinned
`platform-riscv` dependency rather than a test checkout of Vanllia.

## Verification and limits

Use Kernel's `verify` Host runner with the compatible ABI13 source above,
or select the explicit
[manual qualification workflow](../../../.github/workflows/smp-owner.yml)
after merging, and provide a new output directory. `--fixture` is no
longer accepted.
The runner must still pass its complete SMP, Lifecycle, Retirement and
Shutdown marker/receipt checks, normal/native task exit, exact request replay
and failure-path cleanup. Existing source-instrumentation patches retain their
strict SHA-256 preimage and fail-closed application.

`verify smp` defaults to **4 Harts** when explicitly invoked. The other
scenarios allow selecting 1 or 4 Harts; the low-memory/failed-handoff tests
require real denial and retirement evidence. Full SMP qualification stays
outside ordinary CI (except separately opted-in benchmarks). A one-Hart
target smoke test is not evidence for cross-Hart completion, migration,
instruction visibility, cancellation or all-Hart shutdown.
