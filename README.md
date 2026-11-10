# Kernel

Shuyra Kernel is the RV64 microkernel used by Vanllia. It runs isolated
programs and lets them exchange data, delegate access, and signal events
across protection boundaries.

## Documentation

- Read the [design guide](docs/README.md) for the Typst design and its worked
  example of a request crossing two isolation domains.
- Read the [Kernel API](api/README.md) for public contracts and the
  [Client guide](client/README.md) for invoking them.
- Use the [source map](docs/source-map.md) to find the implementation of a
  mechanism.

## Packages

| Path | Responsibility |
| --- | --- |
| [`api/`](api/) | `kernel-api` defines public types, call ABI and boot contracts. |
| [`client/`](client/) | `kernel-client` executes typed calls through the RV64 trap interface. |
| [`server/`](server/) | `kernel` implements the kernel as a library for the platform image. |

## Build and test

Install [Rustup](https://rustup.rs/) and an x86-64 Linux host toolchain with a C
linker. Run these commands from the repository root:

```sh
rustup toolchain install nightly --profile minimal --component rust-src,rustfmt --target riscv64gc-unknown-none-elf
cargo +nightly fmt --all -- --check
cargo +nightly build --locked --workspace --target riscv64gc-unknown-none-elf
cargo +nightly build --locked --workspace --release --target riscv64gc-unknown-none-elf
cargo +nightly test --locked --workspace --target x86_64-unknown-linux-gnu
```

For guest execution, use the [QEMU test guide](server/tests/README.md). Build
the bootable system image through the
[Vanllia RV64 platform](https://github.com/shuyra-arche/vanllia/tree/main/platform/riscv).

## Releases and source

Pin API and Client to the same [release tag](https://github.com/shuyra-arche/kernel/releases)
or exact Git revision. Review the
[API and ABI compatibility notes](api/README.md#source-migration-and-abi-compatibility)
before updating a pin. GitHub releases identify source revisions. The
[Shuyra release standard](https://github.com/shuyra-arche/.github/blob/main/docs/engineering/release.md)
defines the date-based tags. See [Contributing](CONTRIBUTING.md) for review
and verification requirements.

Kernel and API derive from [anreonyr/sqware](https://github.com/anreonyr/sqware)
at `672d8d2deafc8067c94b6af340a5f89c6a765c9c`. Historical Client mechanisms
derive from [Execution](https://github.com/shuyra-arche/execution) at
`1b333a28bc4c34beaea3f7986a9bb6e8ab2db414`. The internal SBI module derives
from [SBI](https://github.com/shuyra-arche/sbi) at
`2dc10bafb919eb2e8f6ca37c95cfd56f9a05831d`, originally extracted from sqware's
`crates/sbi` at `c970afaf7dae1145633278253351be729b625717`.

Git history preserves original authorship and migration records. The source
checkpoints and this repository provide no license grant. Preserve
third-party notices with their code.
