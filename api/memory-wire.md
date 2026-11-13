# Next memory request and result encoding

Status: implemented **value codecs**, awaiting Server/Client integration and ABI
activation. These are the M1 request/result contracts for the settled transitions
in [#142](https://github.com/shuyra-arche/kernel/issues/142), extending the
[fixed-reference and migration contract](memory.md). They do not activate ABI 11
or change the current ABI/Ledger v10. Host codec tests do not prove execution,
request deduplication, live authorization or hardware settlement.

`abi::memory` owns `Request`, `Reply`, `Delivery`, `Opcode`, `RequestId`, `Failure`,
and the action/result variants. `model::flat` owns `BlockPhase`. No new Pend
target, general transaction syscall or second resource-state authority is added.
There are no `RegisterCall`/`KernelCall` implementations for these drafts.

## Invocation and snapshot

The proposed trap number is `(11 << 48) | (domain << 32) | operation`.
`Opcode::from_call_number` rejects other versions and unimplemented slots.
For activation, a0 points to a **256-byte** request and a1 to a **128-byte** reply;
a2–a5 must be zero. Both ranges require checked address arithmetic and caller
memory access. The provider takes one complete request snapshot before acceptance
or output writes, including when the buffers overlap. The byte-copy contract
does not require Rust struct alignment or casting caller memory to a struct.

All words are little-endian u64, including on host tests. Every decoder requires
the exact descriptor length. Native-size identifiers and coordinates must fit
the build host; request IDs, revisions and maintenance record IDs preserve their
full 64 bits. Unused bytes are zero and checked on input. The checked constructor
and decoder enforce the same representation rules.

The first four request words are:

| Word | Meaning |
| ---: | --- |
| 0 | Version = 11 |
| 1 | Domain = Flat 12 or Nest 13 |
| 2 | Operation slot below |
| 3 | Nonzero RequestId for a mutation; zero for InspectResult |

`Request::from_call(number, bytes)` checks the actual trap number against the
descriptor's operation and domain. Entry must use that check rather than trusting
the descriptor to select a different operation. `from_bytes` alone is useful for
stored request data and is not this dispatch check. Current `EnvCall` rejects
these call numbers even when their descriptors are valid.

## Payloads

The following symbols expand inline, without a pointer or Rust padding:

| Symbol | Words |
| --- | --- |
| S | SlotPosition `[flat_pend, flat_id, fixed_page]` |
| M | MemoPosition `[nest_pend, nest_id, mapping_id, fixed_page]` |
| V | Nonzero SlotVersion |
| G | Nonzero MemoGeneration |
| B | BlockRef `[flat_id, record_id, block_generation]` |
| H | Maintenance `[flat_pend, B...]` |
| Optional H | `[0]` or `[1, H...]` |
| Optional B | `[0]` or `[1, B...]` |

An absent optional value has no payload, and the rest of the descriptor is zero.
Only tags 0 and 1 are accepted. Every H still requires the original record's live
branch/Task, range, phase and data/operation rights at execution. It is not a new
authorization root. A staged Slot can belong to a different Flat from B; only
Server's trace of the exact Page and original authority can validate that case.

| Domain / slot | Action | Payload after the four-word header |
| --- | --- | --- |
| Flat 12 / 19 | Occupy | `[S..., V]` |
| Flat 12 / 20 | Vacate | `[S..., V, H...]` |
| Flat 12 / 21 | Block | `[S..., count, Optional B...]` |
| Flat 12 / 22 | Allow | `[H...]` |
| Flat 12 / 23 | Push | `[local S..., local V, remote S..., remote V, Optional H...]` |
| Flat 12 / 24 | Pull | Same encoding as Push, opposite transfer direction |
| Nest 13 / 21 | Insert | `[target M..., G, Source...]` |
| Nest 13 / 22 | Remove | `[M..., G]` |
| Nest 13 / 23 | Publish | `[M..., G, Optional H...]` |
| Nest 13 / 24 | Retract | `[M..., G]` |
| Nest 13 / 25 | Protect | `[M..., G, access]` |
| Flat 12 or Nest 13 / 18 | InspectResult | `[query_tag = 3, requested_id]`; header RequestId is zero |

Create/Delete, Reserve/Release, Split/Merge and object/position observation
variants are not encoded by this revision. Their proposed slots remain in the
migration document; these codecs reject them. This does not retire those v10
operations. The largest currently implemented payload is the maintenance
Push/Pull request: 17 total words (136 bytes); all remaining bytes are zero.

Block's range is nonempty and fixed-origin; byte and coordinate overflow reject.
A recovery B must name the same Flat as S. Fresh Block has no previous maintenance
generation; recovery checks B's generation. The caller's request sequence handles
duplicate acceptance independently of residency. Block's execution must cover the
actual complete physical units and follow historical publications.

Push/Pull reject identical Flat ID and position even when the supplied Pends
differ. Independent grants do not create two unique holdings. Same-Flat distinct
positions are representable; Server still checks both grants and actual geometry.
Protect's access uses READ=2, WRITE=4, EXECUTE=8, requires READ or EXECUTE and
requires READ for WRITE. Representability does not bypass data ceilings, W/X or
backend restrictions.

### Insert sources

| Source tag | Remaining words | Required interpretation |
| ---: | --- | --- |
| 0 | `[S..., V]` | Flat logical position, including Fresh/Missing if valid; check expected geometry/residency at commit |
| 1 | `[share_pend, view_id, fixed_page, count]` | Nonempty Share span measured from the original View origin; preserve underlying source and branch |
| 2 | `[M..., G]` | Capture that Memo's logical source under its generation and original ceilings; never mint an owning Slot |

IDs must be nonzero and within the existing identity range; spans must not
overflow. Source versions are Insert commit checks, not permanent residency pins
for subsequent Publish. Memo source capture must reject cycles or resolve the
original logical binding without creating a new authorization root. Target INSERT
and, for replacement, REMOVE are checked by Server along with current source
authority. This codec does not silently retract an old publication.

## Stored result and delivery are separate

The six-word Reply header is `[11, domain, operation, request_id, outcome, failure]`.
The request ID is always nonzero. The remaining words encode a Value below,
followed by a checked zero tail to exactly 128 bytes. Failure words contain the
two's-complement i64 code in a u64 word.

| Outcome tag | Failure | Value | Meaning |
| ---: | --- | --- | --- |
| 0 Rejected | A known negative cause, except ResultExpired/ReplyFault | None | This attempt made no resource change |
| 1 Pending | 0 | Required | Accepted work remains; observe/advance the same operation |
| 2 Committed | 0 | Required | The operation's documented commit/completion condition was reached |
| 3 Failed | A known negative cause, except ResultExpired/ReplyFault/RequestMismatch | Operation-specific | Accepted work failed; retained hardware/resource responsibility is not erased |
| 4 Expired | ResultExpired (-14) | None | The old result cannot be proved; never execute that identity again |

RequestMismatch rejects this conflicting attempt, not an earlier accepted
operation under the same ID. Do not overwrite its original result with that
rejection. A retained Rejected result can consume a request sequence; retry a
changed attempt with a new sequence. Failed is never a substitute for Rejected.

Value tags and payloads are:

| Tag | Payload | Operation |
| ---: | --- | --- |
| 0 | Empty | Only Rejected, Failed-without-value or Expired |
| 1 | `[slot_version, page_id]` | Occupy/Vacate; zero page means Missing |
| 2 | `[local_version, remote_version, page_id]` | Push/Pull; page identity is nonzero |
| 3 | `[memo_generation]` | Insert/Remove/Publish/Retract/Protect |
| 4 | `[B..., block_phase]` | Block/Allow |

Page IDs are descriptive positive identities at most `isize::MAX`; they confer
no rights. Slot/Memo versions are nonzero u64. A result's Value must match its
operation. Constructors and decoders reject these additional impossible claims:

- Occupy Committed with a zero Page; Vacate Committed with a retained Page.
- Vacate Pending with a zero Page; this would imply release before settlement.
- Pending Occupy, Push/Pull, Insert or Remove. These logical transitions either
  commit atomically or return a rejection/failure, with no exposed partial state.
- Block Pending other than Draining, or Block Committed other than Held.
- Allow Pending other than Releasing, or Allow Committed other than Closed.

BlockPhase tags are Draining=0, Held=1, Releasing=2, Closed=3, Orphaned=4. A Failed
Block/Allow result must retain its known reference and Draining/Releasing phase
respectively, or Orphaned. Failed Vacate must retain its still-resident Page.
Failed Occupy, Push/Pull, Insert and Remove have no result Value: they cannot
claim a partial atomic commit. Failed Publish/Retract/Protect may retain the
known Memo generation. Held does not promise immutable content: the authorized maintenance
actor can still establish controlled writes.

Publish/Retract/Protect may report Pending with their recorded Memo generation.
Committed Retract means the selected publication settled; it does not freeze
other Memo writers. Committed Publish does not promise data-race freedom. A
historic Committed Block/Held result can outlive a later Allow/Closed transition;
the live record still decides permission. None of these replies is a fresh-state
or hardware-proof substitute.

### Register return

`Delivery` encodes a0–a2 as `[0, 128, 0]` when a complete Reply was copied, or
`[failure_code, 0, 0]` when delivery failed. Unknown codes, a wrong byte count,
nonzero reserved registers and positive status values reject. Pending is a Reply
outcome, not an overloaded positive register status.

Read the output bytes only after Written. ReplyFault means copying the reply
failed; it says nothing about whether the operation already committed. Retain the
original RequestId and inspect its record. Server must record the result before
the copy and must not roll back a completed logical commit to fit a delivery error.
Other delivery failures likewise must not be guessed to mean rollback.

### Failure codes

These are next-memory-ABI codes, not aliases for the active FlatFail/CellFail/
NestFail domains:

| Code | Cause | Code | Cause |
| ---: | --- | ---: | --- |
| -1 | Denied | -10 | NotTransferable |
| -2 | Dead | -11 | Unsupported |
| -3 | Busy | -12 | VersionExhausted |
| -4 | OutOfMemory | -13 | RequestMismatch |
| -5 | NotAligned | -14 | ResultExpired |
| -6 | NoRegion | -15 | ReplyFault |
| -7 | Stale | -16 | Invalid |
| -8 | SourceEmpty | -17 | Hardware |
| -9 | DestinationOccupied | | |

Busy in Rejected means no resource transition was accepted. Draining Block or
Releasing Allow instead returns Pending, preserving their effects. ResultExpired
has its own outcome because the original effect is unknown. ReplyFault belongs
to delivery, not the stored mutation outcome.

## Result inspection and bounded retention

Request identity is the actual calling Task plus the caller-chosen monotonic
sequence; no actor field in the descriptor can replace the actual caller. A live
resource Pend is not required to recover the caller's own result after its grant
has been revoked. InspectResult returns the stored mutation's opcode and original
ID, not an Inspect-wrapped second result. An expired query whose old opcode is no
longer known uses Inspect for the requested domain with Outcome::Expired.
`Reply::matches(request)` checks these correlation rules, not authorization.

`RESULT_CAPACITY = 16` specifies the next provider's maximum retained records per
Task across Flat and Nest. It is a contract bound, not an implemented journal in
this change. A provider must allocate/reserve record capacity before effects and
retain canonical request bytes and the last result. Completed/rejected/failed
records may be evicted; Pending records may not. Fill all 16 with Pending work
and the next mutation gets no-effect Busy before admission. Resource cleanup
ownership remains with Flat/Nest/Page even if the result window is destroyed.

The accepted high-water mark never decreases or wraps. Identical retries within
the window return/advance the same operation. A different canonical request under
that ID is RequestMismatch and cannot change the original record. If a requested
ID at or below the high-water mark has no record, return Expired; even skipped
sequence numbers below the mark cannot be treated as fresh requests. If a query
is above the mark and has never been accepted, report NoRegion through Delivery,
not a forged stored mutation result. The two domains share the sequence space;
a query using the wrong domain for a retained record also reports NoRegion.

Output addresses and delivery status do not participate in request equality.
Pending work is not reexecuted as a new operation to make progress. A delayed
completion must match the exact operation/object generation; it cannot reduce
new responsibilities. Caller death may discard only journal storage, never the
objects' unresolved resource/hardware responsibilities. Server integration and
race/failure tests must establish these rules before activation.

## Scope and remaining work

The executable tests under `api/tests/abi/memory_contract/` cover every implemented
action/source/optional-maintenance form, independent byte vectors, malformed
fields, trap/descriptor mismatch, impossible result combinations, full-width IDs,
reply correlation, exact lengths and canonical encodings after byte mutations.
Delivery and stored outcome are separately tested. Existing tests keep active
v10 dispatch rejecting next-ABI calls.

Remaining M1 work includes whole-object and geometry-changing call descriptors,
object/Slot/Memo/Block observation payloads, fault registration and bounded waits,
and backend-specific proof interfaces. The bounded journal, permission checks,
atomic state changes and hardware effects described above require Server work;
the codecs alone do not implement them. All M1–M6 milestones remain open.
