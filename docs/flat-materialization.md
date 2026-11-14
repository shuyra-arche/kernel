# Implicit Flat materialization

`FlatRange::materialize` retains a residency for the complete window before
calling `Flat::materialize_range`. Success still means the entire window is
resident. Failure preserves the already published prefix; retry skips it.
Explicit ABI 11 Flats require Occupy and never allocate through this path.

## Allocation responsibility

Each Fresh position follows this protocol under the same Flat state lock used
by transfer and residency:

| State | Event | Result |
| --- | --- | --- |
| Fresh | Prepare with metadata capacity | Allocating with a non-reused reservation identity |
| Allocating | Another materializer, residency or transfer | Busy |
| Allocating | Matching publication with current geometry, level, owner and open lifecycle | Resident |
| Allocating | Guard dropped after failure/cancellation | Fresh, only if the identity still matches |
| Resident | Materialize | Keep the same Page |
| Missing | Materialize | Missing; never allocate or erase the tombstone |

The reservation borrows the Flat lock, keeping its source alive. The allocating
frame owns every unpublished Page, including an Arc construction failure or a
rejected publication. It releases those resources outside the state lock.
Publication checks the slot identity, coordinate, geometry, Page level, current
owner and Flat lifecycle together. A stale guard neither publishes into nor
cancels a replacement slot. Team closure still transfers the Page owner to
Kernel through the existing shared Owner epoch; a reservation is not a second
physical owner. Task teardown cannot destroy the borrowed Flat while its active
kernel operation still owns the reference.

Metadata capacity is prepared in a separate empty Vec outside the state lock.
After reacquiring it, preparation rechecks the slot and capacity; peers may have
completed, transferred or grown unrelated positions. If still needed, it moves
the current entries into the spare storage without allocation and swaps the
buffers. The displaced empty buffer is destroyed after releasing the guard.
Growth remains geometric. Sorted insertion, cancellation and buffer relocation
still move up to O(number of slots) metadata inside the lock. This change does
not make the storage representation a bounded-time operation.

## Lock and call-site audit

On RV64, `SpinLock` retains a `TrapGuard`, masking local interrupts through
acquisition, waiting and release. Previously one materialization guard covered
the whole window, including Vec reallocation, physical frame allocation and
zeroing (up to a multi-Level Cell), Page metadata and Arc allocation. These
allocator operations and old-buffer destruction now execute without the Flat
state guard; a separate prepare/publication pair protects each Fresh position.

The internal Nest fault/materialize-active path uses this implicit allocator.
Borrowed mapping setup and translation publication call materialize as well,
but do not allocate managed Cells. External source preparation may materialize
an implicit range. The retained legacy association implementation still calls
materialize inside its caller's ledger/COMMIT admission; native ABI 11 does not
dispatch that legacy entry. Removing the Flat guard does not release any outer
guard already held by a caller. `FlatRange::reside` and materialized snapshots
also have their own metadata allocations; those are outside this change.

Host tests exercise the production reservation, slot/transfer/residency checks
and Page destruction with paused concurrent preparation, allocation failure,
prefix retry, stale-slot fault injection, owner closure and geometry rejection.
They do not execute interrupt masking or prove target timing. Native ABI 11
guest acceptance covers the assembled system, but is not a concurrent implicit
Flat guest fixture. No physical-hardware latency bound or measured speed-up is
claimed here.
