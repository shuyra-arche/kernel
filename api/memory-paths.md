# Staged memory path integration

This follow-up to [M5](memory-m5.md) connects the remaining operation paths in
the staged v11 Server. The running ABI, Capsule and Ledger remain v10. It does
not change the boot qualification gate, activate native delegation codecs, or
claim M6 service/image acceptance.

| Path | Implemented behavior |
| --- | --- |
| Flat Delete | Full original DELETE scope and current FlatVersion; irreversible Closing; existing source, provenance and maintenance owners drain; Closed only after every holding is exclusive and safe to return. Physical return occurs outside the Flat lock. |
| Flat Split/Merge | One aligned parent range within the same Flat; immediate 512 children; full RESHAPE scope, actual Held maintenance owner and no overlapping gates; all geometry, revisions, Page identities and maintenance gates commit together. |
| FaultRoute | Full Reservation FAULT scope, actual caller and exact live Sign/Reservation lineages; checked ReservationVersion and FaultEpoch successors; replacement ends old waits. Only freshly issued native Reservation roots receive the new FAULT bit. |
| Fault events and waits | Sixteen retained events; exact pending-event coalescing; fixed first-admission deadlines; non-consuming Poll/finite reads; Task continuations revalidate authority and epoch before delivering. |
| CPU fault recovery | Native reservations enter a bounded continuation; empty/denied/blocked sources do not become nonresident events. Only the accepted successor publication of the original Memo/source, current authority and visible CPU translation can complete recovery. Terminal failures retain their signed code in ExitCause::Fault. |
| External Nest Delete | Exact registered hardware root and expected NestVersion; admission closes before the existing binding keeper drains payload, translation and auxiliary responsibilities. |

Split/Merge retains fixed Flat coordinates through sparse geometry overrides.
Every new head receives the same newly advanced FlatVersion as its SlotVersion;
retired child stamps are never reused. Allocation preparation failure restores
the complete old holding and gate set. A merge cannot discard distinct original
provenance chains. Staged holdings must first return to their original range;
active maintenance publications and foreign or overlapping gates remain Busy.
Existing logical Memo selections must still describe a complete physical unit
after reshaping, so incompatible selections require an ordinary authorized
replacement before publication.

Fault notification uses an existing direct Sign trigger as a coalesced hint.
A private Sign endpoint forwards Reservation, handler-lifetime and exact grant
changes to CPU/read continuations; it conveys no data authority. Queue admission
and sleeper registration share COMMIT. Replacement/Remove invalidates the old
event, and publication acceptance records the exact predecessor and successor
generations. Wake hints, supplied Pages, timeouts and user acknowledgements
cannot complete a wait. Closing records loss before queuing wake delivery,
without running a continuation under a Nest or Reservation lock.

## Verification boundaries

The production storage tests in `server/tests/memory/` cover partial geometry,
fixed coordinates, repeated split/merge stamps, old-version rejection, weak
aliases, overlapping gates, hardware retention and every allocation-failure
position before conversion. Closure tests retain all Pages until the complete
holding set can return and prove that Closed cannot reopen.

`server/tests/fault/` executes the production queue: coalescing, immutable
deadlines, capacity, terminal eviction, expired/invalid cursors, lost-result
rereads, route-generation replacement and preparation rollback. Task authority,
Sign forwarding, scheduler continuation and MMU completion are outside that
host fixture. The invocation harness still uses explicit resource/scheduler
fixtures and checks descriptor/result transactions.

RV64 debug/release builds compile the full integration. The existing QEMU suite
executes v10 guest regressions; it does not exercise native v11 route/delegation
or native fault recovery through the trap ABI. M6 must supply those end-to-end
service, cancellation, timeout, publication-race and image compatibility tests
before activation. SMP, executable qualified access, MMIO, unqualified topology,
ATS/PRI and device replay remain unsupported under the M5 qualification rules.
