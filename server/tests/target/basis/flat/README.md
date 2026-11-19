# Flat target test

The Kernel-owned Flat probe exercises real implicit allocation and materialization,
OutOfMemory recovery, reservation contention, owner cancellation,
Page transfer and version checks on QEMU.

The runner composes the selected source revision with a local Kernel Task probe
in a disposable archive. The test-owned patch is guarded by reviewed source
hashes; no production ABI or permanent test hook is added.

```sh
verify flat --kernel "$PWD" --output /tmp/flat --harts 1
verify flat --kernel "$PWD" --output /tmp/flat-multi --harts 4
```

Normal CI runs one Hart. Four-Hart behavior is opt-in and must not be inferred
from one-Hart evidence.
