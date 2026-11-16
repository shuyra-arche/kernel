# ABI domain split (section 12)

Status: M0 mapping for the unpublished ABI 14 replacement. This document is
the working map for moving `api::abi::memory` from one Flat/Nest operation
enum to independent domain contracts. It does not add a new ABI version: ABI 14
has not been released, so the public types are replaced in place.

## Target shape

| Layer | Target |
| --- | --- |
| Flat operations | `flat::Action`, `flat::Query`, `flat::Value`, `flat::Opcode` |
| Nest operations | `nest::Action`, `nest::Query`, `nest::Value`, `nest::Opcode` |
| Shared request | `RequestId`, `Request<A>`, `FlatRequest`, `NestRequest` |
| Shared result | `Outcome`, `Delivery`, `Reply`, bounded journal |
| Dispatch | entry reads `Domain + slot`, then decodes the domain action |

There is no public unified `memory::Action/Query/Value` and no re-combined
internal business enum at the entry boundary. Wire slots keep the existing
`Domain + slot` numbering; the Rust enums and codec matches are domain-local.

## Current to target operations

| Current `Action` | Target |
| --- | --- |
| `CreateFlat` | `flat::Action::Create` |
| `DeleteFlat` | `flat::Action::Delete` |
| `Occupy` / `Vacate` | `flat::Action::Occupy` / `Vacate` |
| `Block` / `Allow` | `flat::Action::Block` / `Allow` |
| `Push` / `Pull` | `flat::Action::Push` / `Pull` |
| `Split` / `Merge` | `flat::Action::Split` / `Merge` |
| flat `Inspect` / `InspectResult` | `flat::Action::Inspect` / `InspectResult` |
| `CreateNest` / `DeleteNest` | `nest::Action::Create` / `Delete` |
| `CreateSpan` | `nest::Action::Reserve` |
| `DeleteSpan` | `nest::Action::Release` |
| `ReleaseRange` | part of `nest::Action::Release` (`RangeSlice`) |
| `Insert` / `Remove` | `nest::Action::Insert` / `Remove` |
| `Publish` / `Retract` / `Protect` | `nest::Action::Publish` / `Retract` / `Protect` |
| `FaultRoute` | `nest::Action::FaultRoute` |
| nest `Inspect` / `InspectResult` | `nest::Action::Inspect` / `InspectResult` |

## Semantic changes that are not a mechanical rename

- `nest::MemoPosition { range: PendId, page }`: the range Pend is the stable
  authority bound to one internal Span. Range Pend derivation/narrowing must be
  checked at commit; a historical coordinate cannot reach a reused address.
- `nest::Action::Reserve` creates address occupancy and returns the range
  authority. `Release` takes a `RangeSlice`/version, not a public SpanId.
- Flat and Nest queries/results split; both keep the same
  Rejected/Pending/Committed/Failed/Expired semantics and bounded journal.
- The existing Flat rights and Nest rights remain separate; only the envelope
  and result delivery are shared.

## Migration order

1. Add `flat/` and `nest/` type modules and shared `Request<A>`; keep the wire
   slot table fixed.
2. Move request/query/value codecs into each domain and make entry dispatch by
   `Domain`.
3. Migrate client builders, server entry, Pend/View/Flat call sites and tests.
4. Replace `SpanId`-based Nest positions with the Pend-bound range; remove the
   unified public enum and the old field paths.
5. Run fmt, host tests and RV64 debug/release; add target coverage where a
   reviewed fixture exists. No ABI version bump.
