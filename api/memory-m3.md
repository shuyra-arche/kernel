# M3 residency implementation progress

M3 is **in progress**, based on the M2 provider revision
`ee92aa1d69f7939e237cd3b8dfa481c9bd388095`. This first implementation supplies
explicit Flat residency commits and the staged Task result path. It does not
complete persistent Memo associations, versioned Insert or publication migration.
Running ABI, Capsule and Ledger remain v10.

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

## Staged invocation and result storage

`entry::call::next_memory` is compiled but its dispatch branch requires the
running ABI version to equal 11. The current v10 dispatcher rejects every next
call before it reaches that branch. Changing the version alone is not activation
approval: the [M1 gates](memory-m1.md#migration-order-and-acceptance-gates) apply.

The staged path copies the complete 256-byte input before output/effects, binds
it to the actual opcode and rejects realtime callers. It implements CreateFlat,
Occupy, conservative Slot inspection and result inspection. Other operations
return Unsupported. Blocked or hardware-busy Slot observations return failure;
this slice does not fabricate zero responsibility counts for them.

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

- Persistent logical Memo association without retaining a current Page as its
  source; retain the original authority and ceiling across missing residency.
- Atomic Insert replacement with source/target expected versions and INSERT plus
  REMOVE for replacing an association; failure preserves the old association.
- Resolve the current Page at Publish; keep each older publication and its
  reverse tracking bound to its original Page until exact retirement.
- Versioned direct transfer for explicit sources, with first-version rejection
  of unsettled publications, source loss and recovery tests.
- Integrate Vacate's maintenance prerequisites with M4 and complete the remaining
  next-domain/resource handlers before any provider/consumer switch.

## Evidence boundary

Host tests execute the production Flat state and Page using observable allocator
receipts: racing Occupy, stale requests, unchanged versions on rejection,
Missing rejection, exact return, strong/weak holders and hardware retirement.
Production Pend tests check next rights without widening the v10 decoder. The
registration fixture's unrelated Flat family continues to panic if exercised.

These tests do not execute the staged Task dispatcher or prove guest, cross-Team,
fault recovery or physical hardware behavior. RV64 compilation checks the actual
provider code, including its inactive branch, but cannot replace integration.
No QEMU or physical execution is claimed. M3 stays unchecked in #142.
