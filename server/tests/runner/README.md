# Target runner

`verify run --suite <project-directory> --kernel <repository> --output <fresh-directory>`
executes a project discovered from `suite.json`. Target projects own their
source patch contracts, Guest inputs and observable assertions; Runner owns
source capture, build invocation, QEMU or Cargo test execution, timeouts,
process cleanup and evidence.

A QEMU suite can declare an additional Guest/Capsule, imported test helpers,
Cargo features and environment, required result markers and optional distinct
capture evidence. A Cargo test suite names its owning package and test target.
Adding a new project does not change Runner code.

`composition/` contains only the shared patch application mechanism. Source
patches are stored inside the test projects and protected by exact preimage
SHA-256 contracts. The disposable provider is never the working source tree.

On Linux, child processes run in isolated process groups. Leftover descendants
invalidate success; process output, build failures and cleanup receipts are
retained for diagnosis. See `../README.md` for invocation.
