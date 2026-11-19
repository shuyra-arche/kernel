# Host scenario runner

This standalone Cargo workspace selects Kernel-owned target probes. Its Host
library supplies bounded process handling, evidence and Capsule assembly for
those probes. Vanllia's selected Shuyra product uses an independent Native/ABI13
runner under `product/tests/runner/`, without depending on this package as a
Cargo library. System maintains its own native contracts. Kernel uses a fixed `platform-riscv` build dependency, not
a Vanllia product-test checkout or centrally owned test policy.
See the [verification guide](../README.md) for all entry points and ownership.

`composition/*.patch` and sibling JSON maps form explicit source input contracts.
The JSON keys are relative paths and values are SHA-256 hashes of complete
preimages. All hashes must match before Git checks and applies a patch. Review
both the changed source and test assumptions before refreshing a contract.
Patch content is retained with each run. Baseline comparisons reject unreviewed
preimages instead of silently patching a different implementation.

Every Kernel `verify` subcommand uses Kernel-owned Guest and CPU/Platform
test sources. `--fixture` is no longer a recognized argument. The frozen
SMP/Lifecycle/Retirement/Shutdown Guest resides at
[`target/smp/`](../target/smp/README.md); the original ABI13 executable
assertions and request-journal recovery remain intact. Private provider
instrumentation is applied only after exact SHA-256 preimage validation.
The standalone `platform-riscv` Git package is an immutable build dependency.
Ordinary Kernel CI selects Flat, Activity, Room, IRQ and one-Hart Realtime
checks; explicit SMP/multi-Hart verification remains separate.
Vanllia calls the pinned Kernel-owned workflow with the exact Kernel SHA from
its product gitlink, rather than adding this Host runner as a Cargo dependency.
The workflow revisions, selected provider and Cargo-resolved Platform source
are reviewed immutable inputs.

CLI examples and full build/boot commands are recorded in inputs and results.
Results from host runner contracts cover orchestration failure modes; they do
not establish RV64, QEMU or hardware behavior.

On Linux, the shared process helper acts as a child subreaper and places each
command in a process group. It uses the spawned Child's ID as the group ID.
After the leader exits, leftover group members are killed and reaped for at
most one second; cleanup.json records the result. A leader that exited zero
with any leftover child fails as cleanup-failed. Timeouts keep their timeout
status and also record whether their process group settled. The runner gives
direct children PR_SET_PDEATHSIG, but a hard kill of the runner cannot guarantee
cleanup of compiler grandchildren that have already been spawned.
When a nonzero exit leaves any group members or has an unsettled cleanup, the
runner returns an error while preserving the leader's original exit status and
code in result.json. This prevents an exit code from being consumed as a valid
business-negative result after a cleanup failure.

Other Unix hosts retain process-group termination for timeout handling, but
the leftover-process settlement check is only verified on Linux.
