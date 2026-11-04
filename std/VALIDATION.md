# Validation record

Actual check command entry point: `std/verify.sh`. The script sets
`CARGO_TARGET_DIR="$STD/target"`, changes to `$REPO` so the root `.cargo`
config is always discovered, and uses absolute paths derived from the script
location. It is cwd-independent.

Toolchain actually reported by `rustc --version`:

```text
rustc 1.100.0-nightly (a69a63265 2026-09-03)
```

Host test target: `x86_64-unknown-linux-gnu`.
RISC-V target: `riscv64gc-unknown-none-elf`.

## Host test suites

All host suites use canonical API path dependencies and do not run syscalls:

- execution general: 13 passed
- execution unit: 1 passed
- resource capability: 22 passed
- ipc rpc/host: 28 passed
- ipc rack-reader-host: 3 passed
- ipc session-establish: 23 passed
- ipc session-exchange: 15 passed
- rt args host: 1 passed
- std ABI identity: 1 passed

Total: 107 passed.

## RISC-V library builds

- debug default workspace libs: `kernel-std`, `kernel-std-execution`,
  `kernel-std-resource`, `kernel-std-ipc`, `kernel-std-rt`.
- release default workspace libs: same.
- debug rt all features: `entry,panic-handler,global-allocator`.
- release rt all features: same.

## Standalone RISC-V ELF fixtures

- `kernel-std-std-only-elf` debug and release.
- `kernel-std-rt-opt-in-elf` debug and release.

Both fixtures reference memory, task/resource, and IPC mechanisms so final ELF
dead stripping cannot fake absence.

## Symbol evidence

Archive scan:

- all non-rt target rlibs under `std/target/riscv64gc-unknown-none-elf/...`
  (including canonical API env/wire and dependency archives such as talc and
  spinning_top) were scanned for `_start`, `save_args`, `tls_bootstrap`,
  `clean_ret`, `rust_begin_unwind`, `rust_begin_panic`, `__rust_alloc`,
  `__rust_dealloc`, `__rust_realloc`, `__rust_alloc_zeroed`, and `__rg_*`
  allocator shims: none found.
- runtime default rlib has no registration symbols.
- runtime all-feature rlib has `_start`, `save_args`, `tls_bootstrap`,
  `rust_begin_unwind`, and allocator shims; `clean_ret` is absent because it is
  generated in the consuming binary by `entry!`.

Final ELFs:

- std-only: has its own `_start`; no `save_args`, `tls_bootstrap`, or
  `clean_ret`.
- rt-opt-in: has `_start`, `save_args`, `tls_bootstrap`, and `clean_ret`.

External symbol-guard probes recorded in
`/workspace/std-evidence/symbol-guard-probes.json` confirmed:

- clean mechanism archive passes,
- missing archive fails,
- corrupt archive fails,
- known rt all-feature rlib fails the mechanism-absence check with
  `_start`/TLS/panic/allocator shims.

## ELF entry and PT_LOAD evidence

For debug and release rt-opt-in ELFs, an embedded Python ELF64 parser checked:

- executable type and RISC-V machine,
- `e_entry == _start`,
- every `PT_LOAD` has nonzero page-aligned `p_align`,
- every `PT_LOAD` has page-aligned `p_offset` and `p_vaddr`,
- `p_offset` and `p_vaddr` are congruent modulo 4096.

## Metadata and dependency identity

`cargo metadata --locked` on the std workspace verified:

- exactly one canonical `kernel-api-env`, `kernel-api-wire`, and
  `kernel-api-macros` package, each under `api/...`;
- no old `env`, `wire`, or `mold` package identities in the std workspace
  graph;
- std-only fixture has no resolved path to `kernel-std-rt`;
- no `libc` or `cc` package in the resolved graph.

## Source provenance evidence

`PROVENANCE.tsv` was regenerated with real tab delimiters and relative
`std/...` current paths. It covers the same 59 imported Rust sources as the
external audit: 52 unchanged, 3 modified, 2 moved unchanged, 2 moved modified.
The moved linker script and unchanged start/args bytes are represented there
and in `PROVENANCE.md`.

## Limitations

- No QEMU and no syscall execution. This validation does not itself publish,
  merge, push, or request GitHub approval; a later Draft PR is a separate phase.
- Host suites are compile/behavior tests with local fixtures; they do not run
  target syscalls.
- This prototype is not Rust's actual platform `std`/PAL/Cargo implementation
  and is not a production cutover.
