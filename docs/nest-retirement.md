# CPU Nest withdrawal and invalidation

`Nest::withdraw` coordinates the following owners under **Layout → Nest →
Span** lock order. Preparation and commit retain those locks; hardware
invalidation runs after releasing them. No new global transaction owner or
retirement ledger is introduced.

| Owner | Preparation / failure | Infallible commit | Settlement |
| --- | --- | --- | --- |
| Layout / Occupancy | Validate exact Reservation generation and complete interval coverage; reserve every cut and the entire non-reusing ticket-ID batch. Failure changes no occupancy. Abandoning preparation can consume unused IDs. | Cut live intervals into retired intervals using reserved IDs. Existing overlapping tickets keep their identity. | Reclaim only tickets claimed from the settled prefix. |
| NestState / CPU table | Prepare equivalent page-table geometry and reserve retired-table storage. Failure does not withdraw a translation. | Clear prepared PTEs and retain emptied table pages. | Covering CPU proof releases tables outside the Nest lock. |
| Memos / Publication | Count affected publications and reserve retained-publication capacity. | Remove publications into Retirement; exact Page instances remain retained and retired. Logical associations have their own lifecycle. | The exact covering proof settles physical publication responsibility. |
| Span / Index | Prepare remaining ActiveRanges and replacement index node. | Replace active ranges and index node while holding Nest and Span locks. | A retained Span cannot disappear before its publications settle. |
| Retirement | Reserve retained Span and ticket capacity. | Retain resources/tickets and include required global coverage in Dirty. | Match domain instance, attempt, ASID and coverage before making tickets claimable and releasing resources. |

All ticket identities are reserved atomically by `Serial::reserve`. In
particular, insufficient identity space for two fresh intervals cannot retire
the first interval and then fail on the second. Capacity and ID preparation
finish before the first occupancy or PTE withdrawal; the commit does not allocate.

`Retirement::begin` admits only Dirty or Failed. The initiator records its exact
request under the Nest lock, then owns one synchronous backend invocation.
Concurrent Fencing observers return `NestError::Busy` without polling or
replacing the request. `ready_state` makes at most one invalidation attempt per
turn and rechecks open admission; `close` similarly refuses a newly dirty batch
and never reopens admission. A later operation owns its own continuation, rather
than inheriting another operation's result.

Kernel buffer copies validate and walk the current software PTEs while retaining
Nest/Span locks, then use the permanent physical mapping. They do not depend on
the Task's cached translations and do not initiate or wait for a fence. This
keeps COMMIT-protected buffer access pure and avoids treating unrelated Fencing
as a denied user buffer; removed entries and current permission limits still
refuse access. Failed or closing address spaces refuse the copy.

Creating a standalone CPU Nest or a Team with a fresh Nest installs trampoline
translations during preparation. Those paths prepare and clean up outside
COMMIT, then recheck live creation/management authority under COMMIT before
publishing. Private strong references outlive that final guard so registration
failure cannot trigger last-owner hardware cleanup inside COMMIT. Debug builds
assert the fence boundary is outside COMMIT.

Page-fault and envcall entries already preserve the original instruction and
yield on Busy through the existing deadline retry mechanism. Explicit
`retry_invalidation` retains the same Nest, settles at most four address tickets
and four queued cleanup nodes per turn, and rearms remaining cleanup. Consumed
Span cleanup remains in the preallocated Nest cleanup chain. Task/Team retirement
and caller-owned failed operations remain retryable; no Busy/error response
proves that a Page or address is safe to reuse.

An SBI/backend error leaves Failed with every retained resource and ticket.
Retries use a fresh attempt and preserve the required scope. Stale or insufficient
proofs fail closed. Closing only reaches Closed after the covering proof; elapsed
time, cancellation, source exit and business completion cannot substitute for it.

The CPU backend performs a finite Hart scan and synchronous SBI RFENCE calls.
This change removes software waiting on a peer's Fencing state; it does not
establish a physical time bound for firmware that does not return. A stalled
firmware invocation retains its resources and initiator. Host scripted proofs
verify ownership and identity matching; QEMU executes the target shootdown and
withdrawal paths, and neither establishes physical-hardware timing/quiescence.
