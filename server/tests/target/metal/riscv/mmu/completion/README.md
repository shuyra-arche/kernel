# Native CPU SMP verification

Build the host utility first, then run from the Kernel root:

```sh
cargo +nightly build --locked --manifest-path server/tests/runner/Cargo.toml --target x86_64-unknown-linux-gnu
verify=server/tests/runner/target/x86_64-unknown-linux-gnu/debug/verify
```

This provider-owned composition selects the CPU cases of Vanllia's native ABI 11
fixture at `50ebf0feaae16374ba431f83a515fe6ce273c8b7`. It archives an exact Kernel
HEAD and the fixture, patches API/Client/Server to that one provider, verifies reviewed composition preimages and retains each applied patch. The paired
platform qualifies only 128 MiB coherent QEMU virt RAM, with no External route,
executable Publish, passthrough or hotplug.

Run with the root README's toolchain and QEMU 10.0.x/OpenSBI 1.6:

```sh
export QEMU_RISCV64=/path/to/qemu-system-riscv64
export QEMU_OPENSBI=/path/to/fw_dynamic.bin
"$verify" smp --kernel "$PWD" \
  --fixture /path/to/vanllia --output /tmp/native-smp-debug
"$verify" smp --kernel "$PWD" \
  --fixture /path/to/vanllia --output /tmp/native-smp-release --profile release
```

Commit the provider before running: the runner deliberately tests its exact HEAD.
It runs 1/2/4 Harts with multi-thread TCG and a 120-second limit per boot. Source
trees, both lockfiles, ELFs, build output, firmware hash, serial logs and result
metadata remain in the output directory. Raw logs belong in CI artifacts.

The selected existing cases exercise native RW publication, authority withdrawal,
source lineage, maintenance, Block/Vacate and Task cleanup. `cases.rs` adds a
writer that repeatedly accesses a real mapping while its owner retracts or
releases it. A retained observer alias must stop changing at the successful
retirement receipt. The test replays that exact request, closes the observer,
vacates the Page and verifies zeroed fresh allocation. Separate assertions
require executable Publish and Protect to return Unsupported under data-only
qualification. No guest fence is used to manufacture Kernel completion.

Writers sleep before retirement to exercise placement across Harts. A test-owned
observer in the disposable provider records their actual user traps; the runner
requires both writer Task IDs to have executed on distinct Harts in SMP runs.
`probe.rs` additionally withholds one real Hart across two bounded failures,
injects a duplicated old acknowledgement, checks retained Page responsibility,
and completes a fresh round. This boot probe tests real fences and bookkeeping;
the guest writers separately test real translations and accesses.
`instructions.rs` repeats the withheld/old/duplicate acknowledgement test on
the independent instruction bank, with real FENCE.I before acknowledgement.

The composition adapts the historical fixture's immediate-success assumptions:
rejected Busy admissions retry within a deadline; accepted Block/Memo failures
replay the original request and explicitly continue the returned Block or Memo
generation. Recovery never overwrites the failed journal entry or treats it as
permission to reuse memory. The complete adaptations are in `patches/*.patch`.

Physical hardware, executable SMP, and complete System/Vanllia #39 acceptance
require separate evidence. The System #12 runtime at
`7e4f9dc19bdaf4a0e95ff3b557fdc8959b8547d7` is an additional consumer scenario,
not a replacement for these Kernel-owned tests.

Run the unchanged System source with the published Twine Release recovery
(`7c75c62d15d173032d99fed54ca7fd00337d787b`, Twine #10) as a consumer regression:

```sh
cargo +nightly run --locked --manifest-path /path/to/vanllia/tests/runner/Cargo.toml --target x86_64-unknown-linux-gnu -- system --kernel "$PWD" \
  --fixture /path/to/system-at-7e4f9dc --twine /path/to/twine-at-7c75c62 \
  --output /tmp/system-native-runtime
```

This archives all three exact revisions, selects the pinned Twine source,
and builds the release guest and platform with the same API/Client/Server.
It requires all four original runtime markers and clean exit on 1/4 Harts within
the original 120-second limit. In particular, a retry observer must not steal
the CPU batch between the heap owner's accepted Publish and its completion.
The build graph, ELFs, Capsule and serial results are retained; no System source
or fixture failure policy is adapted. Twine explicitly continues a retained
Release(Busy) with a new request while preserving the original failed receipt;
its owner is returned only after the new completion succeeds. This is the
issue's component fixture, not full
System or Vanllia product acceptance.

Reviewed source inputs must match the SHA-256 contracts under
`server/tests/runner/composition/`. An incompatible historical baseline fails
explicitly and requires a separately reviewed composition contract. See the
[verification guide](../../../../../README.md) for evidence and CI ownership.
