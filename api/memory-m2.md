# M2 organization and authority acceptance

M2's next-interface organization and shared Server authority mechanisms are
implemented in [PR #146](https://github.com/shuyra-arche/kernel/pull/146), pending
review and merge after M1 (#145). This milestone does not activate ABI 11.
The [M1 activation order](memory-m1.md#migration-order-and-acceptance-gates) still
requires M3–M5 before a coordinated provider/consumer/image switch.

Section 14 of #142 adds [Reservation and Pend acceptance](memory-reservation.md)
to this same round. The names below reflect that revision; M2's original delivery
does not prove the added source-export, distribution or lifecycle scenarios.

## Acceptance matrix

| #142 M2 requirement | Delivered implementation | Compatibility boundary |
| --- | --- | --- |
| Public Cell operations belong to Flat/Nest | `abi::memory::{flat,nest}::Call` and `kernel_client::memory::{flat,nest}` are the next interface. Frame admits only version 11, domains 12/13 and the frozen operation slots; Cell domain 2 and alternate creation cannot decode. Server Cell entry only parses v10 descriptors and delegates to Flat or Nest | The separately documented v10 API/Client adapter remains until the coordinated switch; there is no v11-to-v10 fallback |
| Align with #141 | Whole versus Reservation scope, exact Reservation identity and full narrowed windows are checked by the existing live Pend owner; `Reservation::position` also matches token and Nest identity | Whole cannot impersonate a Reservation grant; no new Reservation authorization root |
| Remove duplicate creation | Flat owns the one logical allocation/source preparation path; the v10 Slot-returning adapter calls it, while the next interface has only Flat Create | The legacy return representation survives only for ABI 10; no additional physical owner is created |
| Remove cross-Team dependence on Task-local handles | The Flat transfer mechanism takes `Endpoint::Flat(SlotPosition)` and resolves both grants in the actual caller's ledger. It has no SlotId, remote Task or scheduler argument. The v10 adapter normalizes a Flat-backed handle once into that same fixed position. Flat/Share association directly constructs Source without a temporary SlotId | Private/Share-root v10 handles have a separate compatibility endpoint; the next wire cannot express it |
| Preserve both endpoint authorities | Both fixed positions resolve under COMMIT, then resolve again before admission. Rights, data ceilings, complete geometry, original source and exact Page identity are checked before flights. Source retirement gates retain authorization across the holding transaction | Request versions and v11 maintenance/result policy attach at the later resource mutation layer; an Endpoint alone does not authorize a v11 mutation |

## Responsibilities and lifetime

The holding engine and atomic transaction now live under
`basis::flat::{transfer,transaction}`. Page allocation and hardware retention
remain their own internal mechanisms; moving a holding does not adopt PageOwner.
The entry domains own their behavior: Flat owns holding/geometry adapters, Nest
owns association/publication/inspection. `entry::call::cell` contains only the
old wire adapter and cannot become a second implementation of those operations.

Flat/Share association retains the actual caller, original Pend branch, immutable
source window and data ceiling. Hold has no public identity; the legacy handle
table alone assigns SlotId. A per-Task weak discovery table separately retains
cleanup reachability, bounded to 1024 distinct live sources. Known sources can be
reused at capacity; new sources fail before association, without evicting an old
cleanup obligation. Task/source authority loss still follows the existing Nest
withdrawal path. A weak record itself is neither permission nor retirement proof.

## Invocation and recovery boundary

The Client snapshots the canonical 256-byte request, creates a distinct zeroed
128-byte result buffer and invokes one transport. The register Frame checks spare
registers and address arithmetic; descriptor decoding binds the copied bytes to
the actual operation. Overlapping caller buffers are legal at the wire boundary:
the provider must finish its snapshot before effects or output.

Delivery and the operation's stored result remain separate. The Client checks
the exact result kind and correlation with the original request, including result
inspection returning an older mutation opcode. Failed delivery, malformed output
or a stale response never triggers automatic retry, rollback, a new RequestId or
a fallback to Cell. A returned Rejected/Failed/Pending is an operation result,
not a transport error.

## What remains before activation

Running EnvCall, Ledger and Capsule remain version 10. The new Client requires
the future coordinated provider; the current dispatcher rejects its calls.
This is a deliberate activation gate, not a claim that v11 syscalls run today.

M3–M4 must connect the next request dispatch and Task journal to authoritative
resource versions, explicit Occupy/Vacate, persistent logical Memo sources,
atomic reassociation, maintenance records, fault queues and retained failures.
The M2 movement engine preserves the running v10 holding semantics; the v11
mutation layer must additionally apply the frozen expected-version checks and
first-version restriction against moving unsettled publications. M5 must supply
physical evidence before enabling hardware paths. M6 verifies the assembled
image and performance. Only then remove the v10 adapter during the coordinated
version switch, never silently reinterpret its descriptors.

## Evidence

- API tests check exact Flat/Nest routing, rejection of Cell and wrong versions,
  register padding, address overflow, snapshot length and opcode substitution.
- Client tests use an explicit host transport to check one invocation, canonical
  buffers, loss of delivery, malformed success, stale replies, result recovery,
  exact Reservation observations and mutation/observation separation.
- Production Pend/State tests check fixed ranges after Narrow, full units,
  Whole/Reservation separation, independent endpoint rights, TRANSFER without
  ASSOCIATE, ancestor revocation, holder death and data suspension.
- Production weak-registry tests check bounded cleanup discovery independently
  of public handles. Existing Page/storage tests continue to cover holding
  transitions and rollback. All production call/transfer paths compile for RV64
  with the normal target flags and `-Dwarnings`.

Host tests do not execute the Task ledger, complete cross-Team transfer or Task
exit on RV64. No QEMU or physical execution is claimed here; this environment
does not have `qemu-system-riscv64`. M6 remains open. Review this evidence as
organization/authorization and protocol evidence, not proof of a running v11
provider or physical platform.
