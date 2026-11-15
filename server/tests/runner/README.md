# Host scenario runner

This standalone Cargo workspace selects Kernel-owned target probes. The shared
Vanllia execution library supplies bounded process handling, evidence and Capsule
assembly. Native memory and System consumer scenarios live in Vanllia, not here.
See the [verification guide](../README.md) for all entry points and ownership.

`composition/*.patch` and sibling JSON maps form explicit source input contracts.
The JSON keys are relative paths and values are SHA-256 hashes of complete
preimages. All hashes must match before Git checks and applies a patch. Review
both the changed source and test assumptions before refreshing a contract.
Patch content is retained with each run. Baseline comparisons reject unreviewed
preimages instead of silently patching a different implementation.

The fixture pin must match the immutable Git dependency revision. Changing it
requires the linked Vanllia provider PR and assembled QEMU evidence. Library
fetchability from a PR branch does not mean that provider PR has merged.

CLI examples and full build/boot commands are recorded in inputs and results.
Results from host runner contracts cover orchestration failure modes; they do
not establish RV64, QEMU or hardware behavior.
