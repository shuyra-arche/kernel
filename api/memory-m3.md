# M3 residency implementation progress

M3 is **in progress**, based on the M2 provider revision
`ee92aa1d69f7939e237cd3b8dfa481c9bd388095`. The implementation supplies explicit
Flat residency, versioned holding transfer, persistent logical Memo association,
atomic Insert/Remove and the staged Task result path. Publication entry/result
integration and maintenance prerequisites remain unfinished. Running ABI,
Capsule and Ledger remain v10.

The same-round [Reservation migration](memory-reservation.md) updates the public
names and scope selectors used below. Its additional authorization acceptance
is tracked separately from the existing M3 residency evidence.

## Delivered core

- A next-provider Flat is created without allocating payload pages. Its private
  compatibility marker prevents the v10 implicit materializer, holding transfer
  and reshaper from modifying it. This marker is not a selectable sharing mode;
  it separates provider generations until the v10 adapters are removed.
- The existing Flat state owns residency and sparse Slot stamps. Fresh heads
  start at one. Successful Occupy/Vacate assign the next Flat revision to the
  changed Slot; untouched heads keep their stamp. Checks and failed preparation
  do not advance either revision. Exhaustion rejects before changing holdings.
- Occupy takes a new owned Page, not a retained Arc snapshot. The entry prepares
  and zeroes outside COMMIT, then rechecks live OCCUPY, complete geometry, source
  admission and expected SlotVersion before installing the holding. A losing
  preparation is released. Missing never becomes Fresh or receives implicit zeros.
- The Vacate storage commit checks both Page hardware accounting and exclusive
  software ownership, including weak upgrade paths. It leaves Missing and returns
  the unique allocation for destruction outside the Flat lock. It cannot succeed
  while another reference, publication, quarantine, change or residency survives.
  Reference counts alone are never evidence of hardware completion.
- Next Flat roots carry the frozen additional rights. Live Pend selection checks
  their exact bits, scope and ancestry; unrelated/unknown bits reject. The v10
  bitflag decoder and delegation encoding are unchanged. Next-version Pend
  decoding/attenuation still needs the coordinated provider migration.
- Push/Pull recheck both complete endpoints and Slot versions under COMMIT and
  ordered Flat locks. Page admission closes before checking exact hardware
  obligations; unsettled publications reject without changing either holding or
  version. A same-Flat move advances its revision once and stamps both heads.
  Moving into Missing restores the same Page and allocation owner without zeros.

## Logical association and physical publication

Next Insert stores a fixed logical Source and reverse binding, without retaining
a current Page. It accepts missing residency, retains the original actor/branch
and sticky access ceiling, and rechecks live authority on use. A descriptive
source coordinate survives revocation but supplies no authority. Selection from
another Memo requires Reservation PUBLISH and keeps that Memo's original lineage.

Insert checks the target generation and requires INSERT plus REMOVE when
replacing an association. It prepares binding/index capacity before the atomic
swap; stale, busy, denied or exhausted requests preserve the old association.
Remove advances generation without moving a holding or starting hardware work.
Next logical operations reject affine mappings and v10 captured-Page Memos.

The shared publication core resolves a logical association's current resident
Page when publishing; it does not materialize Fresh or Missing. Each publication
and retirement freeze retain their exact physical Page independently of the
logical source. Source cleanup discovers that retained Page, not a newly
resolved source holding. A settled logical Memo releases its physical freeze
and keeps the source binding; the v10 captured-Page/Ready behavior is preserved.
One logical Memo's retirement no longer requires unrelated CPU writers to
retire. Existing Page admission still coordinates an in-flight change; this is
not yet the complete next-version Publish/Retract protocol.

## Staged invocation and result storage

`entry::call::next_memory` is compiled but its dispatch branch requires the
running ABI version to equal 11. The current v10 dispatcher rejects every next
call before it reaches that branch. Changing the version alone is not activation
approval: the [M1 gates](memory-m1.md#migration-order-and-acceptance-gates) apply.

The staged path copies the complete 256-byte input before output/effects, binds
it to the actual opcode and rejects realtime callers. It implements CreateFlat,
Occupy, Push/Pull without maintenance, Insert/Remove, conservative Slot and quiet
logical Memo inspection, and result inspection. Other operations return
Unsupported. Blocked or hardware-busy Slot observations and busy Memo observations
return failure; this slice does not fabricate responsibility counts or published
access snapshots for them.

Each actual Task owns one lazy boxed Journal with the existing 16-record bound.
Mutation admission reserves its result before resource effects. Resource commit
and terminal result recording share COMMIT; caller output occurs afterward.
Replay returns the stored result without executing again. Output faults do not
erase committed results or reopen the original RequestId. Ledger serialization
precedes the journal and COMMIT; no nested ledger acquisition is needed.

The Vacate wire branch has an explicit all-refusing maintenance gate. The tested
storage primitive is **not a working Vacate syscall**: M4 must validate the live
Held BlockRef, original branch/Task, exact Page, overlapping obligations, and
consume maintenance-owned retention in the same commit before that gate opens.

## Remaining M3 work

- Wire next Publish/Retract/Protect to Task results with Pending/Failed retention
  recorded before hardware effects, without holding COMMIT across CPU fences.
  They currently return Unsupported; the atomic dispatch path must not classify
  a partial hardware failure as a no-effect rejection.
- Supply exact busy/published observations, remaining Nest lifecycle handlers,
  and live source-loss/revocation integration tests through actual entry paths.
- Integrate Vacate's maintenance prerequisites with M4 and complete the remaining
  next-domain/resource handlers before any provider/consumer switch.

## Evidence boundary

Host tests execute the production Flat state and Page using observable allocator
receipts: racing Occupy, stale requests, unchanged versions on rejection,
Missing rejection, exact return, strong/weak holders and hardware retirement.
Production Pend tests check next rights without widening the v10 decoder. The
registration fixture's unrelated Flat family continues to panic if exercised.

The Memo harness executes the production Memo state and publication receipts.
It covers logical persistence, replacement failure, exact physical retirement,
independent writers, unfinished publication, and v10 compatibility. A combined
Flat/Memo test moves and restores a holding while preserving its logical binding.
Its Source registry and publisher routes are explicit inert fixtures: they do
not prove live authority, reverse-index integration or hardware completion.

These tests do not execute the staged Task dispatcher or prove guest, cross-Team,
fault recovery or physical hardware behavior. RV64 compilation checks the actual
provider code, including its inactive branch, but cannot replace integration.
No QEMU or physical execution is claimed. M3 stays unchecked in #142.
