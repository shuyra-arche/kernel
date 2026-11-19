# Host scenario runner

This standalone Cargo workspace selects Kernel-owned target probes. Its Host
library supplies bounded process handling, evidence and Capsule assembly for
those probes. Vanllia's selected Shuyra product uses an independent Native/ABI13
runner under `product/tests/runner/`, without depending on this package as a
Cargo library. System maintains its own native contracts. Kernel reuses a
reviewed Vanllia platform fixture, not Vanllia's test policy.
See the [verification guide](../README.md) for all entry points and ownership.

`composition/*.patch` and sibling JSON maps form explicit source input contracts.
The JSON keys are relative paths and values are SHA-256 hashes of complete
preimages. All hashes must match before Git checks and applies a patch. Review
both the changed source and test assumptions before refreshing a contract.
Patch content is retained with each run. Baseline comparisons reject unreviewed
preimages instead of silently patching a different implementation.

The fixture pin identifies the published Vanllia source still required by SMP,
IRQ, Realtime, lifecycle, retirement and shutdown Guest compositions. Flat,
published ABI13 Activity, and `verify room --mode room` instead use Kernel-owned
Guests and standalone Cargo-pinned Platform packages; these reject `--fixture`. The ABI13 Activity
composition retains its own exact Kernel preimage contract. Other private
scenarios continue to reject unreviewed Vanllia fixture inputs.
Ordinary Kernel Host/RV64 checks do not execute that target composition;
Kernel's separate Flat and pinned-target QEMU workflows can select it.
Vanllia calls the pinned Kernel-owned workflow with the exact Kernel SHA from
its product gitlink, rather than adding this Host runner as a Cargo dependency.
The workflow revision and platform fixture are reviewed immutable inputs.

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
