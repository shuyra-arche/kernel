# Kernel-owned ABI13 IRQ target

The IRQ Guest and asserted transport behavior come from Kernel revision
`789ffd19b5359f3de172fb9aad8a61f6a7113a64`, and the standalone QEMU bootstrap from the reviewed
Vanllia revision `f2dfa14f5b1f7bd47ee84841ba5c7112e8475950`. This is a **test-only**
bootstrap, not an exportable production policy or a new ABI.

The Guest imports the existing frozen ABI13 `pinned/support.rs` and
`pinned/stack.rs` from Kernel's Room target tests. The test Platform imports
the same checked `pinned/platform/device.rs` and `window.rs`; it does not
check out, copy, or patch Vanllia's product memory-test workspace. The normal
`platform-riscv` package is still an immutable Git build dependency.

The `uart_boot.rs` function adapts only the trusted QEMU UART authority
description. The granted UART remains a static mapping of the initial Team.
This preserves the production Kernel's live IRQ/Sign/Pend authorization,
rebind, claim/complete, driver exit, retirement and delivery paths. It is
never a general claim about physical device safety, hotplug or SMP.

The source-only Kernel latency-window patch remains protected by exact
preimage hashes for the selected published ABI13 provider. Normal CI runs
Debug and Release on one Hart; optional performance studies are separate.
