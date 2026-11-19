# CPU completion, migration and retirement verification

Kernel owns the real CPU-only SBI/translation-completion target cases.
The public ABI13 Guest, its executed permission/memory assertions, the
CPU-only QEMU Platform, and unchanged recovery cases are now composed from
[`target/smp/`](../../../../smp/README.md) rather than from a
historical Vanllia product-test checkout.

The private tests keep the complete published ABI13 behavior: real User
mapping writes during Retract/Release, immutable observer mappings after
retirement completes, Block/Vacate and zeroed fresh allocation, replay of the
same failed request and retained owner before subsequent retry, rejected
executable Publish/Protect, and actual Task migration.

The real-Hart [`probe.rs`](probe.rs) withholds one CPU across bounded
retries, injects duplicate/late acknowledgements, and requires a fresh
completed round without prematurely releasing retained Page obligations.
[`instructions.rs`](instructions.rs) verifies the distinct FENCE.I bank;
the test must not substitute a guest-only fence or mock completion for the
Kernel's real synchronization and native effects.

## Run explicitly

Build the standalone Host runner with the root README's declared toolchain:

```sh
cargo +nightly build --locked --manifest-path server/tests/runner/Cargo.toml --target x86_64-unknown-linux-gnu
verify=server/tests/runner/target/x86_64-unknown-linux-gnu/debug/verify
"$verify" smp --kernel /path/to/kernel-at-b1062f60 --output /new/smp --profile release
"$verify" lifecycle --kernel /path/to/kernel-at-b1062f60 --output /new/lifecycle --cancel --pressure --harts 4
"$verify" retirement --kernel /path/to/kernel-at-b1062f60 --output /new/retirement --cancel --harts 4
"$verify" shutdown --kernel /path/to/kernel-at-b1062f60 --output /new/shutdown --quiet --harts 4
```

The explicit `smp` command defaults to **four Harts**. Other commands must
specify their requested Hart selection. Paired QEMU uses coherent 128 MiB
virt RAM, QEMU 10.0.x and identified OpenSBI/DBCN firmware; no device or
External DMA resource is admitted. A boot has a 120-second outer timeout;
failure, ambiguous completion, fatal output, missing native receipts and
incomplete cleanup fail closed. Full raw build/boot evidence is preserved in
the new output directory, including exact source and Platform identities,
linker, Cargo lock, tool/firmware hashes, output and statuses.

The [manual qualification workflow](../../../../../.github/workflows/smp-owner.yml)
is **workflow_dispatch only**. Ordinary PR/push CI does not schedule SMP or
multi-Hart target campaigns; the one-time nine-scenario one-Hart migration
verification establishes runnable ownership, **not** four-Hart migration,
instructions, cancellation or all-Hart shutdown safety. An explicit
four-Hart qualification run remains necessary and must retain its raw
result; it is not to be inferred from Host tests or one-Hart QEMU.

## Scope

Kernel private completion and lifecycle suites are separate from the Vanllia
product's Native ABI13 admission tests, and System's native Runtime/Toolchain
acceptance is owned by System. Only the exact reviewed published ABI13 Kernel
is admitted by this suite. Kernel main/ABI14, physical CPU/IOMMU/device
safety and entire assembled-product acceptance remain separate requirements.

The provider patch contracts under
[`runner/composition/`](../../../../../runner/composition/)
must match complete SHA-256 preimages before disposable instrumentation is
applied. An incompatible provider revision fails explicitly; it is not a
reason to remove a check or add an ABI compatibility shim.
