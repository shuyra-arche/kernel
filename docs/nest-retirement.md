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

## RV64 multi-Hart completion

The SMP backend uses a conservative global invalidation round. It invalidates
caches of translations, not peer mappings or authority. `Retirement` still owns
the exact Nest instance, ASID, attempt, PTE edits, Pages and detached tables.
There is no second resource ledger. Logical withdrawal closes admission under
the existing COMMIT/Nest boundary before clearing PTEs; the caller drops those
guards before starting a round. Kernel direct-memory copies retain
their software mapping/ownership locks independently of user translations.

The participants are **every Hart ever marked online**, including idle Harts,
kernel continuations and Harts that previously ran a Task before it migrated.
ONLINE is sticky. An unresponsive or stopped participant is not removed to make
a round succeed. An offline Hart has never accessed a user address space. Its
first online publication is followed by `FENCE RW,RW`, SATP activation and
`SFENCE.VMA` before scheduling: paired with the initiator's fence before scanning
ONLINE, a newly admitted Hart either participates or starts from the new PTEs.
Every later Task return also executes the existing SATP/SFENCE handoff.

After publishing PTE/payload edits, an initiator obtains a non-reusing round
from one atomic sequence. AcqRel allocation chains earlier initiators' edits
into later rounds. Each participant's requested round is a monotonic maximum;
an Acquire observation of a later round therefore covers all earlier edits.
The responder executes `FENCE RW,RW; SFENCE.VMA; FENCE RW,RW; FENCE.I`, then
Release-publishes the **observed** round. SFENCE excludes future accesses using
old translations. The second explicit data fence orders prior loads/stores
before the acknowledgement; the initiator's Acquire observation orders resource
reuse after those accesses. Neither an IPI nor an SBI remote SFENCE reply is a
substitute for this sequence.

All participants must acknowledge this round or a later covering global round.
Late and duplicate acknowledgements cannot regress a completed round or settle
a newer one. Sequence exhaustion rejects new work rather than wrapping. These
hardware rounds are not public IDs: only the successful invocation creates the
existing exact `Completed<Request<Asid>>`, which Retirement revalidates under
its own lock before releasing anything.

Responders allocate nothing and take no locks. They run at trap entry/return,
after SSIP is cleared (including the IRQ-off idle loop), and in existing lock
wait reception windows. Simultaneous initiators service each other's requests
while polling; no global owner lock serializes their waits. Caller-only Team
operation/Pend ledger serialization can remain: responders never acquire those
locks, and even a peer queued on them services its mailbox in the lock wait.
COMMIT/Nest/Page locks do not cross initiation. Polling ends after one second of the admitted timebase
or five million iterations, whichever comes first. IPI submission errors, timeout
and partial completion return failure with no receipt. These bounds cover Kernel
polling, not a firmware call that never returns. A late response is harmless;
a retry receives a fresh round and still owns the original retained resources.

Data visibility relies on the separately supplied complete coherent normal-RAM
fabric and the publication fence before PTE exposure. Unknown fabrics remain
unsupported. Native executable SMP Publish/Protect remains **unsupported**:
`with_local_instructions` is rejected at installation on SMP, and data-only
qualification cannot admit RX. The responder's conservative FENCE.I does not
qualify instruction-fetch topology or the executable PTE exposure boundary.
Page-wide W/X exclusion and External/IOMMU draining conditions remain intact.

ABI, Ledger and Capsule stay 11/11/2. API and Client have no changed descriptor
or wire result. The existing result journal retains committed or failed-with-
retained-value results before delivery; replay never repeats an accepted effect.
A failed retirement result does not authorize reuse: inspect the same logical
resource and retry its retained cleanup through the owning lifecycle. This
implementation does not turn failed CPU results into asynchronous success.

The owning [SMP test composition](../server/tests/guest/smp/) records exact source,
fixture, lock, toolchain, command and image hashes. Host receipt tests exercise
software ordering and retention only. QEMU execution is separate evidence and
does not qualify physical hardware, executable SMP or device quiescence.
