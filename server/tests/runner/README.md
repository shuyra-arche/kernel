# Host scenario runner

This standalone Cargo workspace selects Kernel-owned target probes. This package’s host library supplies bounded process handling, evidence and
Capsule assembly. Vanllia consumes it for composition verification; Kernel does
not depend on Vanllia’s tooling. Native memory and System consumer scenarios live in Vanllia, not here.
See the [verification guide](../README.md) for all entry points and ownership.

`composition/*.patch` and sibling JSON maps form explicit source input contracts.
The JSON keys are relative paths and values are SHA-256 hashes of complete
preimages. All hashes must match before Git checks and applies a patch. Review
both the changed source and test assumptions before refreshing a contract.
Patch content is retained with each run. Baseline comparisons reject unreviewed
preimages instead of silently patching a different implementation.

The fixture pin identifies the published Vanllia source used by explicit
private-probe integration runs. Reviewed preimages reject incompatible fixtures.
The ordinary Kernel Host/RV64 CI never fetches or executes that composition.
Vanllia’s consumer pins this package and the provider to published full SHAs;
changes require linked PRs and assembled QEMU evidence, with Kernel first.

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

Other Unix hosts retain process-group termination for timeout handling, but
the leftover-process settlement check is only verified on Linux.
