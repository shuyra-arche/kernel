# Implicit Flat target probe

This standalone RV64 composition checks the real implicit Flat allocator,
reservation, Page and transfer implementation. ABI 11 user Calls create explicit
Flats, so the native user guest cannot enter this allocation path. The probe
runs trusted Kernel Tasks without adding a public ABI or production test hooks.

`run.py` archives the provider's exact HEAD and the pinned Vanllia platform into
a new output directory. In that disposable copy it selects `provider.rs` as the
crate root, attaches the test cases as a child of Flat, starts a Kernel Task at
boot, and inserts guard entry/exit callbacks into the real SpinLock guard. It
records the complete provider composition diff and suite hashes. Production
source and visibility in the checkout remain unchanged. The platform pin and
removal of its historical `memory-v11` feature match the native CI composition.

Use Python 3.12+, the root README's Rust target, QEMU 10.0.x and OpenSBI with SBI
DBCN. Set `QEMU_RISCV64` and `QEMU_OPENSBI` as in the native guest. From the Kernel
root, with the fixture checkout at the pin in `run.py`:

```sh
python3 server/tests/guest/flat/run.py --kernel "$PWD" \
  --fixture /absolute/path/to/vanllia --output /tmp/flat-debug
python3 server/tests/guest/flat/run.py --kernel "$PWD" \
  --fixture /absolute/path/to/vanllia --output /tmp/flat-release \
  --profile release --rounds 3
```

Both commands select 1 and 4 Harts, 128 MiB RAM and multi-thread TCG. A boot
passes only with all case markers, normal Task shutdown and no fatal report.
Each boot has a 120-second timeout; internal waits have a 30-second deadline.
The checked-in standalone lockfile and production target flags are preserved.

## Contract cases

- Exhaust the real 2 MiB allocator, release one block and materialize two Cells.
  Check one Resident prefix survives repeated OutOfMemory, with unchanged Page
  identity, physical address and data; after releasing memory, retry completes
  with a distinct, zeroed second Page and no abandoned reservation.
- Hold a real allocation reservation while a peer attempts materialization,
  residency and transfer. Check consistent Busy. Four-Hart runs force that peer
  onto a different Hart; one-Hart runs interleave ordinary Tasks. Close the
  shared Owner epoch before publication and check Kernel adoption.
- Cancel after actual physical allocation, return the unpublished Page exactly
  once, observe the worker's Task exit, and check a later retry allocates fresh
  identity. The counted allocator forwards real frame receipts and asserts that
  its allocation and return execute with local interrupts enabled.
- Race four materializers for eight rounds. Check one published Page identity,
  transfer claim/rollback and commit, preserved identity/data, and sticky Missing
  after transfer and reverse-claim cancellation.

The Owner closure case uses the production shared epoch, not a complete user
Team destruction scenario. Cancellation drops the in-flight operation before
Task exit; it does not inject asynchronous stack destruction. Stale identity
and geometry fault injection remain in the host memory suite. No test certifies
physical hardware timing or public SMP memory support.

Workers use no-argument entries, matching the existing observation Task. During
fixture development, concurrent creation with stack arguments hit
`stack page materialized before args write`, and repeated creation overlapped
retirement and hit `kernel frame not mapped` or Shootdown failure. All worker
stacks/frames are therefore prepared at boot before secondary Harts start;
workers are released and exit normally during the cases. This probe does not
validate concurrent Kernel-Nest Task creation or qualify SMP memory draining.
Kernel Task logs use the existing byte backend because buffered
console output intentionally rejects their low-half supervisor stack pointers.
All reporting occurs outside measured intervals.

## Optional timing comparison

Timing is exploratory and is not part of the required correctness check. Run
baseline and candidate sequentially, without another test or build running:

```sh
python3 server/tests/guest/flat/run.py --kernel /absolute/path/to/baseline \
  --fixture /absolute/path/to/vanllia --output /tmp/flat-before \
  --mode timing --profile release --rounds 3
python3 server/tests/guest/flat/run.py --kernel "$PWD" \
  --fixture /absolute/path/to/vanllia --output /tmp/flat-after \
  --mode timing --profile release --rounds 3
```

The same suite builds against the pre-reservation provider in timing mode.
Each boot materializes 64 fresh 2 MiB Cells, dropping each before the next.
Only guards whose lock address is that Flat's state contribute to `flat-hold`;
`flat-call` measures the complete materialization call. Records are fixed-size
per-Hart atomics, with no allocation, logging or new lock in the callback.
Count, sum, maximum and 32 base-2 histogram bins use firmware clock ticks. Bin
zero contains zero ticks; bin `i` contains `[2^(i-1), 2^i-1]` ticks for `1..30`;
bin 31 includes all larger values. Convert with the reported ticks/second.

The measured guard interval starts after acquisition and ends before unlock.
It excludes lock acquisition wait and callback recording cost. Both providers
receive identical callbacks. Reception windows and host/firmware pauses can
occur inside a hold: elapsed lock ownership is not uninterrupted IRQ masking.
The single active caller isolates zeroing cost; the contract mode covers
contention separately. This is neither a steady mixed-workload benchmark nor a
WCET argument. Report all rounds and outliers, including whole-call cost.

Each output retains inputs, tool versions, CPU quota/affinity, exact commands,
source/fixture identities, suite/firmware/lock/ELF hashes, composition diff,
build output and per-boot serial/stderr/status. Keep raw evidence as CI
artifacts or attached review assets rather than committing logs to source.
