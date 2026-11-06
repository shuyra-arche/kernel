# Contributing

Follow the [Shuyra contribution standard](https://github.com/shuyra-arche/.github/blob/main/CONTRIBUTING.md)
and its [engineering conventions](https://github.com/shuyra-arche/.github/tree/main/docs/engineering).
The root README documents local packages, toolchain and verification commands.

Use a short-lived branch from `main` and the organization PR template. Keep a PR
draft while implementation or required verification is in progress. Record the
actual toolchain, commands, results and skipped checks; distinguish host tests,
RV64 compilation and guest execution. Core API changes require explicit source
and ABI compatibility review. Merge through a reviewed PR, using squash.

API is one package. Client and Server consume it directly; Server does not depend
on Client. Keep shared semantic types in `model`, call representation in `abi`,
and startup contracts in `boot`. Keep compile-time generation under `tools` and
language runtime/std support outside Kernel. Update the root lockfile whenever
the package graph changes. Preserve source attribution and third-party notices.
