# Kernel target runner

`verify` runs Kernel-owned Flat and Nest target checks against an explicitly
selected Kernel source tree. It assembles a disposable provider, starts QEMU,
checks the Guest's own completion markers, and records source/ELF/lockfile,
build, execution, timeout and process-cleanup evidence.

`composition/` contains only source patches required by the active Flat and
Nest probes. SHA-256 preimages must match before applying them. Test-specific
assertions belong to the corresponding Guest, not to the generic runner.

On Linux each child executes in its own process group. Unexpected survivors
are killed and invalidated, including when the original command exits zero.

See `../README.md` for commands and supported execution modes.
