# Nest, Span, Memo and Translation ownership

This note records the ownership split implemented for #220. It is the
navigation point for the current kernel layout while the remaining
cross-repository and partial-release work is still in review.

## Authority table

| Owner | Owns | Does not own |
| --- | --- | --- |
| `Nest` | Admission, address organization, occupancy, Span index, Team attachment | A position's generation/publication, another Span's private state |
| `Span` | Original range, active access ranges, local Memos, Fault/Export, close phase, shared Translation handle | Address reuse decisions, another Span's Memos |
| `Memo` | One position's generation, logical association, Publication instance, in-flight transition and settlement | Querying the Nest, driving hardware, another position |
| `Translation` | Root/backend, PTE edits, invalidation batches and retained obligations | Memo business state, source authorization, the Span index |
| `Pend / Flat / View` | Live authorization, logical/physical sources and View relationships | Publication ownership inside a Span |

`Layout.occupancy` is the address-occupancy authority. `SpanIndex` locates and
keeps the corresponding `Span`; it is not a second mutating occupancy ledger.

## Lock order

The steady-state order is:

```text
COMMIT
  -> Address
    -> Translation
      -> Span
        -> Page / source
```

- `Address` is the outer Nest lock: layout, occupancy, Span index, Team.
- `Translation` is the shared hardware domain/retirement lock.
- `Span` covers local active ranges, Memos, Fault and Export discovery.
- Hardware completion and Page drops happen after releasing these guards.

A Span holds an `Arc<Translation>`; Translation does not hold Spans strongly.
`Retirement` keeps weak mapping handles while a batch is pending, then upgrades
them once under the fence completion and moves the strong handles into
`Fenced` for lock-external release.

## Vertical path

```text
Insert -> Publish -> Retract -> Settle -> Remove -> Protect
```

- Insert/Remove: `Span::insert` / `Span::remove`
- Publish: `Span::publish`
- Retract: `Span::retract` / `retract_forced` / `retract_polling`
- Settle: local Memo plus the shared Translation batch
- Protect: `Span::protect`
- Inspect/source lookup/snapshot: `Span::inspect`, `logical_source`, `snapshot`

`Nest` remains a narrow dependency for Span lookup/ownership, external
backend preparation/submission, invalidation progress and authoritative
hardware fence selection. It does not mutate a position's Memo.

## Close, drain and reclaim

```text
Span::close       admission/phase -> Closing, faults closed
Span::drain_one   one bounded local position step, resumable cursor
Span::withdraw_exports
Nest::reclaim     remove index entry, release occupancy, phase -> Gone
```

`drain_one` advances at most one position. The cursor moves only after a
clear; a blocked position is retried rather than skipped. The synchronous
`DeleteSpan` call still drives the loop to completion, but the primitive is
ready for a future worker/repoll path.

## Partial release (staged)

`Occupancy::release_range` and `Span::release_range` implement the hole
primitive:

- free a page-aligned subrange of one exact generation;
- keep left/right occupancy slots under the same generation identity;
- remove the range from active access and refresh the index from remaining
  active ranges;
- refuse while a publication, source or translation batch still owns it, and
  refuse to release the last active range.

`Action::ReleaseRange` (Nest slot 27) is the userspace entry. It requires the
`DELETE_SPAN` authority, an exact expected Span revision and a range inside
the authority window. The Span stays open and its revision advances; full
close remains `DeleteSpan`. The operation is additive within ABI 14, so the
existing version and consumer pins stay valid.

## Evidence boundaries

Host tests exercise local Memo state, retirement batches, occupancy holes and
the logical transition helpers. They do not prove RV64 PTE/TLB behavior, QEMU
or hardware completion; those require the RV64 target and the external
integration suite.
