# Published ABI13 Activity owner fixture

Kernel owns this frozen Guest, platform bootstrap and Activity assertions
for the selected published Kernel revision `789ffd19b5359f3de172fb9aad8a61f6a7113a64` (ABI13).
This is **not** a claim about Kernel main/ABI14, a new production ABI, or SMP
qualification.

The Activity Guest, stack, probes and source contracts originate from the
exact Kernel revision above. The test-only support, linker and firmware
bootstrap originally came from Vanllia `f2dfa14f5b1f7bd47ee84841ba5c7112e8475950`,
with the already-reviewed `activity-support` and `activity-platform`
transformations incorporated here. Kernel's private Guest no longer requires
a checkout of Vanllia's product test workspace.

`platform-riscv` remains an ordinary exact-commit Git build dependency.
No runtime platform provider source has been copied. On the supported
QEMU `rv64,svade=off,svadu=off` CPU the test Platform admits coherent
CPU activity. With `ACTIVITY_UNSUPPORTED` and Svade enabled, it intentionally
does not admit that qualification. All other Kernel source/preimage and native
completion verdicts remain fail-closed.
