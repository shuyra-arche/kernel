# Remove kernel event logging

Removed the per-hart event ring, allocation reservation, initialization and
failure variant, event enums, hot-path event calls, panic ring rendering and
per-event JSON export. Obsolete trace-specific host tests are removed with the
removed mechanism. Spare exhaustion and reclamation checks remain; the obsolete
ring-residency requirement is replaced by the retained crash-dump budget check.

Console exceptions, task fault detail and crash backtraces remain. Optional
semihosting exports an explicit crash report only, with bounded lock acquisition
and latched delivery failures. The crash export host test retains those failure
checks. Kernel panic shutdown now uses the SBI System failure reset reason,
matching the existing boot-error path, so removal of the log does not turn a
panic into an ordinary shutdown request. See the
[SBI system reset contract](https://github.com/riscv-non-isa/riscv-sbi-doc/blob/master/src/ext-sys-reset.adoc).

Validation uses installed nightly-2026-10-09 rather than the unchanged historical
toolchain pin. RISC-V checking and the native crash-export test passed. Guest
execution, SMP acceptance and performance measurements have not been run.
