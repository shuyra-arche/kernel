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
