# ReleaseRange

`Action::ReleaseRange` (Nest domain slot 27) releases a page-aligned subrange
of one live Span without deleting the Span.

## Wire

The request has six words after the call number:

| Word | Meaning |
| --- | --- |
| 0 | live Pend token (`DELETE_SPAN` authority) |
| 1 | NestId |
| 2 | SpanId |
| 3 | expected SpanVersion |
| 4 | page offset within the Span |
| 5 | page count |

The request is still the canonical 256-byte memory descriptor. Existing
operation slots and layouts are unchanged, so this is additive inside ABI 14.

## Entry checks

- The caller is live and holds a Pend entry with `DELETE_SPAN`.
- The Pend scope is exactly this Span.
- The requested window lies inside the delegated authority window.
- `Span.version() == expected`.
- `offset + count` is within the Span's initial page count.

Violations return `Denied`, `Stale`, `NotAligned` or `Dead`.

## Span checks

Under Address -> Translation -> Span:

- the Span is Open and has no implicit source;
- no Translation retirement batch is pending for it;
- no published Memo and no in-flight/busy Memo lies in the window;
- releasing does not remove the last active range (use `DeleteSpan`).

A violation returns `Busy` (or `NoRegion` when no active range overlaps) and
changes nothing.

## Commit

On success:

1. the occupancy subrange is released under the same generation;
2. the released pages leave the Span's active ranges;
3. the index key/coverage is refreshed from the remaining ranges;
4. the Span revision advances.

The Span keeps its original identity and range, but cannot access the hole.
A new Span may reserve and index the released pages; a stale address credential
cannot release a newer generation.

The reply is `SpanLifecycle { phase: Open }` with the new revision. A pending
or Closed lifecycle is rejected by reply validation.

## Limits

- Source-backed Spans still require a full close; partial release refuses while
  an implicit source exists.
- The last active range cannot be released partially.
- Host tests cover occupancy holes, active-range splits and the reply contract.
  QEMU/target-level behavior and cross-repository pins still need the external
  integration run.
