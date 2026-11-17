# ReleaseRange

`nest::Action::ReleaseRange` (Nest domain, slot 27) releases a page-aligned
subrange of a live Span without deleting the Span. The public request identifies
the range by its current Pend grant, not by separately supplied NestId/SpanId.

## Wire

The canonical request remains 256 bytes. Its four-word header is:

| Header word | Meaning |
| --- | --- |
| 0 | ABI version |
| 1 | Nest domain (13) |
| 2 | ReleaseRange slot (27) |
| 3 | RequestId, scoped by the actual caller Task |

Immediately after the header are **four payload words**:

| Payload word | Meaning |
| --- | --- |
| 0 | live range Pend token (`DELETE_SPAN` right) |
| 1 | fixed-origin page offset within the original Span |
| 2 | nonzero page count |
| 3 | expected `RangeVersion` |

The first three payload words are `RangeSlice` (24 bytes), followed by
`RangeVersion` (8 bytes). All remaining bytes must be zero. The descriptor
is checked against the actual trap number before admission. A request's
`PendId` is a locator, **not** proof of authority.

These coordinates reflect the current Pend-bound request codec. Do not combine
older pinned ABI 14 clients that still send NestId/SpanId fields with this
provider revision: the wire fields changed during pre-release development.
Rebuild API, Client, Server and the image together.

## Entry checks

- The actual caller is live and owns a live range Pend with `DELETE_SPAN`.
- The Pend scope resolves to one stable Span, including its internal
  `SpanId` disambiguation; an old grant cannot select a newer occupant.
- The requested fixed-origin slice lies inside the current delegated window,
  and `offset + count` is valid within the Span's original page count.
- `RangeVersion` matches the current Span revision and can advance.
- The checked request and result journal are admitted consistently.

Violations reject as appropriate (`Denied`, `Stale`, `NotAligned`,
`Dead` or a resource-specific failure).

## Span checks

Under the actual `Address -> Translation -> Span` synchronization path:

- The Span remains Open, and no covering Translation retirement batch is pending.
- No published or in-flight/busy Memo occupies the requested window.
- The slice overlaps an active range and does not remove the final active
  range; full close is performed through the distinct `Release` action.
- Any existing implicit source remains owned by the Span for its remaining
  active ranges; it does not authorize access through a released hole.

An ineligible or busy request changes no occupancy or access state.

## Commit

On success, the same authoritative commit:

1. removes the selected occupancy interval under the exact original identity;
2. excludes the released positions from the Span's active access;
3. refreshes the Span index against the remaining active ranges;
4. advances the Span revision.

The historical Span retains its original identity and geometry, but cannot
access or release the hole. The returned Nest-domain value is
`SpanLifecycle { range, version, phase: Open }`, where `range` names the
requesting Pend. A new Span may reuse only the genuinely released address
interval. Old grants and late completions never act on the replacement.

## Limits and verification

The operation currently refuses to release the final active range. It also
refuses published, in-flight and retirement-owned positions; it is not a
general asynchronous forced-unmap operation. Do not confuse Pend.Return with
memory range release.

Host tests protect wire fields, range identity, occupancy holes, active-range
splits and result matching. They do not establish RV64 TLB or IOMMU completion;
those still need target and integration evidence.
