# Published ABI13 Realtime target

The real Realtime Guest remains in `../realtime.rs`, owned by Kernel.
This standalone Cargo workspace compiles that file directly; it does not copy
it or fetch any Vanllia product tests.

The same frozen ABI13 `pinned/stack.rs`, `pinned/support.rs`,
`pinned/platform/main.rs`, device/window helpers, build scripts and immutable
Cargo provider pins are used by the sibling Room target. The manifest paths are
explicit; no production code, private feature or ABI is introduced.

Single-Hart QEMU confirms the existing **unsupported admission** contract.
Full fixed-Hart budget/replenishment, priority preemption, Freeze and overload
assertions execute only in the explicitly requested four-Hart mode. As decided,
that SMP campaign remains outside ordinary CI. The runner requires full
source/guest verdict and exact shutdown evidence for every requested boot.
