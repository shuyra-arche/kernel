# ABI 14 CPU translation retirement

This Kernel-owned Guest verifies real CPU access, retained publication
obligations, checked request replay and address reuse on the selected
Kernel ABI 14 candidate. Its QEMU platform declares only coherent CPU RAM;
no device, IOMMU or executable-publish qualification is inferred.

The bounded stack and ABI helpers are an owner-maintained extraction from the
Native ABI 14 acceptance implementation, but this scenario exercises Kernel
CPU completion rather than copying Vanllia product test selection. The
instrumented CPU completion and migration probes remain under
`server/tests/target/metal/riscv/mmu/completion/`.

For the current #220 verification slice, `verify nest` runs the Guest against
one exact Kernel revision using a reviewed `nest-provider` patch contract.
Debug and Release single-Hart are ordinary PR checks; four-Hart execution is
manual, never an ordinary CI matrix. One-Hart passing does not establish
multi-Hart invalidation, migration, physical IOMMU or device drain.

The original ABI 13 published fixtures, their exact provider hashes and pinned
workflow remain immutable. No fallback path accepts a mismatched ABI or source
hash. Kernel #301/#302 are not considered merged solely because this scenario
passes on their temporary combined verification commit.
