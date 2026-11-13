# Reservation migration and section 14 acceptance

This is the same-round follow-up required by [#142 section 14](https://github.com/shuyra-arche/kernel/issues/142#issue-14),
revised on 2026-11-13. It follows M1/M2 and the M3 baseline
`bdb3b74d71f8b6b7b53bfb23378701d8ff87a52e`. The original M1/M2 delivery does not
complete the additional authorization and integration requirements below.

## Names and ownership

Reservation is the stable identity and lifecycle of a Nest address reservation.
It can exist without a Page or PTE. Memo is a position within that fixed-origin
reservation; publication retains a particular physical Page and generation.
Reservation adds no Pend Target, call domain or second resource-state owner.

| Previous source spelling | Current spelling |
| --- | --- |
| `model::cell::MappingId` | `model::nest::ReservationId` |
| `NestScope::Mapping` | `NestScope::Reservation` |
| `NestRights::mapping()` | `NestRights::reservation()` |
| `MappingVersion`, `MappingPhase`, `MappingStatus` | `ReservationVersion`, `ReservationPhase`, `ReservationStatus` |
| `MemoPosition::mapping()` and descriptor `mapping` fields | `reservation()` and `reservation` |
| `Query::Mapping`, `Snapshot::Mapping` | `Query::Reservation`, `Snapshot::Reservation` |
| `Value::MappingLifecycle` | `Value::ReservationLifecycle` |
| `SignSource::MappingPublished/MappingRetired` | `SignSource::ReservationPublished/ReservationRetired` |
| Server Mapping grant/root and namespace registry | Reservation grant/root and `ReservationRegistry` |

There are no old-name aliases or alternate enum variants. Request, reply,
observation, fault and Sign selectors all use the same Reservation identity.
The Server still owns one Nest namespace and the same live Pend lineage.
Cancelling an unpublished address allocation is distinct from releasing a
published Reservation by identity. MMU/IOMMU translation operations, their
mapping errors and hardware retirement records retain their actual terminology.

## Compatibility

This is a Rust source API change. Source consumers of the old names must update
and rebuild; it is not a compatibility alias. Numeric labels, field widths,
scope tag 3, and Sign tags 9/11 are unchanged. The existing v10 adapters also
use the canonical Rust names without reinterpreting their wire requests.

ABI, Ledger and Capsule remain v10. The staged memory descriptors still require
11; a request with a v10 or mixed call/payload version is rejected. No consumer
pin or image activation is changed. The [M1 coordinated activation gates](memory-m1.md#migration-order-and-acceptance-gates)
still apply, including next Pend rights encoding and removal of the v10 Cell
and Share Open/Shut adapters. Existing v10 bitflag decoders continue rejecting
the staged OCCUPY/VACATE/RESHAPE/FAULT additions.

## Evidence and remaining requirements

| Section 14 requirement | Evidence in this follow-up | Still required |
| --- | --- | --- |
| Reservation vocabulary across API/Client/Server | Model, descriptor codecs, scope, grant/root lookup, Sign selectors, fault payloads, registry, diagnostics and docs use Reservation; no old public alias | Review before activation; historical migration names are intentional |
| Joint Target/Scope/Operations validation | Host test exercises 168 combinations through actual PendInfo decoding; original scope, range, unknown-bit and geometry tests remain | Next-version Pend additions and coordinated boot/image tests |
| Whole/Reservation separation and creation lineage | Existing production Pend/ledger preparation tests now use Reservation names, retain real creator ancestry and parent limits, and reject ordinary cross-scope attenuation | Actual Reserve entry and hardware lifecycle integration |
| Fixed identity, coordinate and correlated observations | Golden wire tests retain numeric layout; a different ReservationId at the same address/revision rejects old observations; fixed-origin Narrow tests remain | Actual Release/re-reserve with stale Pend, fault, waiter and request delivery |
| Read-only and non-distributable multi-hop sources | Existing attenuation tests remain; no claim that source export is complete | Audit and enforce live distribution qualification on Memo/Share export, with original and intermediate lineage restrictions; race with Narrow/Revoke |
| Complete resource controls and independent permissions | Existing scoped Pend/registration tests remain | Complete §14.8 Task, multi-reservation, fault, maintenance and CPU/External retirement scenarios |

The new identity-correlation tests execute codecs, not actual address reuse.
Registration tests execute production Pend ancestry and ledger preparation with
Namespace/hardware fixtures. Host checks and RV64 builds do not establish guest,
QEMU or physical retirement behavior. No additional issue checkbox is completed
on the strength of naming changes alone. M3–M6 remain open.
