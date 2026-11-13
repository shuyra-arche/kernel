# M4 admission, retirement and maintenance delivery

This is the staged provider follow-up to [M3](memory-m3.md), based on
`717a9f9fc6034ba1a2bb2af7f40cbf866f4c336b` (PR #150). It implements the
maintenance publication and retirement path described by issue #142 sections
10, 13.2 and 14.7. Running ABI, Capsule and Ledger remain v10; the next provider
remains inactive. Source delivery and host evidence are distinct from guest or
physical hardware acceptance.

## Authority and admission

The existing source RangeBlock owns the fixed Flat window, actual Pend owner,
original Task and non-reusable BlockRef epoch. Task death, branch revocation or
loss of the required complete BLOCK window invalidates the credential without
removing the record. Ordinary Pend Narrow currently refuses a branch owning a
block; the epoch also checks its rights/window defensively at each use.

Maintenance Publish uses the ordinary Memo Publication and Page Retention path.
It checks the actual calling Task, live source lineage, original data ceiling,
Reservation PUBLISH and complete geometry. Internal maintenance publications
must target the caller's Team Nest. External publications retain the existing
live source loan and trusted Hardware validation. Protect of an existing
maintenance publication also checks the maintenance Task; Retract remains an
independent operation on the selected Memo.

A physical permit identifies one retained Page block, not merely the presence
of a Change. Registration and adding an overlapping block use the same gate.
A later block closes new maintenance admission; an earlier accepted receipt
remains counted until exact settlement. Removing a block does not allow its old
permit to impersonate a successor. The source window independently excludes
other branches' blocks, including a block accepted before its Page scan finishes.

Memo owns each maintenance use and its physical receipt. The use remembers its
exact record, epoch, source and cleanup destination. Weak reverse lists discover
cleanup but do not own a second publication state or grant authority. CPU fence
completion and External readiness are followed by live maintenance revalidation.
The External check distinguishes a legitimate source loan from revocation.

## Holding movement and staging

Credential-bearing Push/Pull check both complete TRANSFER endpoints and both
Slot versions under COMMIT. The exact Page must have no outstanding physical
publication. Storage prepares destination and staging metadata before committing
both holdings. A failed version, allocation, geometry or obligation check leaves
the holdings and existing stage metadata unchanged.

A staged Slot retains the original RangeBlock, exact PageId and original fixed
coordinate. The original block weakly discovers these stages. A later overlapping
block finds the historical Page even when the original Slot is Missing, retains
its physical gate and drains its maintenance users. Returning the Page removes
the stage metadata; incoming replacement Pages join the block before transfer.
No physical allocation owner changes.

Staged access requires the original block owner's live ASSOCIATE/data ceiling
in addition to the selected Slot's source authority. Staged Vacate requires
VACATE at both the selected Slot and original block range. A maintenance stage
cannot be exported as a new Memo-source authority. Pure staging and initial
replacement import require TRANSFER at both endpoints; they do not additionally
require ASSOCIATE, writable data or distribution permission.

Every transfer atomically carries the actual donor Pend, actor, fixed window
and operation/data/distribution ceilings with the holding. This provenance is
independent of temporary stage metadata: it survives Allow and later ordinary
transfers. Publish/Protect require every original branch's live ASSOCIATE and
data permission; export and use by another actor require original distribution
permission; Vacate independently requires original VACATE. A transfer-only
grant can move a holding without granting any of these other actions. The
chain is bounded at 64 hops and source/publication discovery lists at 1024
entries; exhaustion rejects preparation before changing either holding.

Returning to an exact original source and fixed window through its actual
branch or real ancestor restores the pre-transfer chain. A sibling grant,
another Flat or shifted coordinate cannot erase provenance. Returning an
initially imported replacement also releases that import's own maintenance
gate, after the successful holding commit. Other blocks remain effective.

Original-source Narrow, revocation and Block discover moved publications through
weak reverse bindings. Memo retains the exact provenance of its physical
publication until settlement, independently of the current holding. Publication
preparation acquires nonblocking operation leases for original sources to
serialize with Narrow; no source lock crosses a hardware fence. These records
retain restrictions and discovery routes, not a second live authorization root.

## Retirement, failure and takeover

Allow acquires the source operation lease, enters Releasing under COMMIT and
closes new maintenance publication before withdrawing its own uses. Up to four
uses are advanced per attempt, outside source/record/Nest state locks. Each uses
the exact retained Memo identity; a Protect generation change does not detach
its maintenance responsibility. Failure or unfinished hardware keeps the Page,
record and gate. The caller must return or explicitly Vacate outstanding staged
Pages before Allow can close the phase. Cleanup transfers/Vacate remain available
in Releasing, while new maintenance Publish is refused.

Only after physical obligations, prepared uses and stages settle does Allow
remove its own record. It neither clears another block nor republishes clients.
A Block/Allow race cannot reopen a Releasing or removed record. Kernel branch
retirement discovers and withdraws maintenance uses even after authority dies;
it leaves native orphan blocks closed.

Recovery resolves an existing orphan by exact BlockRef. The new live Pend must
be an actual ancestor of the old owner and cover the whole original window.
Old uses and physical obligations must settle before ownership changes. The
new epoch is checked against overflow and recorded in the bounded request
journal at the ownership commit, even if later work fails. Old credentials
permanently fail. Recovery preserves the original staged data ceiling and the
captured provenance restrictions, including subsequent rights/data/distribution
narrowing. Only dead original grants in the recovered source window can retarget
to the actual ancestor; siblings, unrelated managers and numerical handle copies
cannot supply ancestry. A legitimate External data loan does not orphan the
maintenance owner or remove its BLOCK/INSPECT management permission.

CPU readers/writers and External readers/writers have distinct checked counters;
maintenance is an exact tag on each non-clonable Retention. Preparation rollback
cancels only that receipt. Published/retiring receipt loss retains the count and
quarantines the Page. The existing conservative exclusion of External writes is
unchanged: this does not claim M5 support for concurrent device writers.

Block observation separates ordinary and maintenance physical obligations and
reports quarantine or discovered failed Memo/fence state. A live original branch
or real ancestor with INSPECT and complete coverage can observe an orphan;
observation neither transfers ownership nor settles hardware.

A failed bounded attempt retains its accepted value. Replaying its RequestId
returns the same terminal result; continuing work uses a new RequestId and
current versions. No reply-copy failure or missing notification rolls back
committed ownership or clears a hardware responsibility.

## Verification and limits

| Evidence | Coverage |
| --- | --- |
| Production Page/Retention host tests | Independent maintenance CPU writers and External readers, conservative device-write exclusion, overlapping blocks, stale physical permits, racing registration/block arrival, checked counter exhaustion, lost completion quarantine |
| Production Flat storage host tests | Exact Page movement with stage/provenance metadata, stale and hardware-busy rollback, allocation-failure atomicity, provenance surviving gate release and later movement, allocator return once |
| Production Publication host test with scripted Hardware | Shared CPU/External maintenance accounting, partial backend submission, wrong completion batch rejection, exact individual completion |
| Production Epoch/Pend/Carried host tests | TRANSFER-only capture, independent data/distribution/disposal restrictions, live Narrow and Task death, actual ancestor recovery preserving later restrictions, exact return coordinates, External-loan management liveness, non-wrapping epoch |
| Production Memo state host test | Exact publication provenance retained through withdrawal until physical settlement |
| Production Journal host test | New takeover epoch retained through later failure and replay |
| RV64 debug and release builds | Complete actual provider integration, including inactive entry paths, with production warnings denied |

The host storage/Memo fixtures use opaque Stage/Provenance metadata and do not
prove the full provenance graph or Stage permission checks. Carried tests use
production Pend permission and ancestry logic; Epoch tests use the existing
Task lifetime fixture and do not settle hardware. Scripted Hardware completion
is not an MMU/IOMMU fence. Native entry, cross-Task dispatch, source-discovery
retirement, staged source authority and full service composition are
target-compiled, not end-to-end executed by these fixtures.

Verification uses the root README commands: formatting, locked workspace host
tests, RV64 debug and release builds, plus `git diff --check`. The final delivery
has 416 host tests passing and 14 hardware tests ignored. No QEMU or physical
run is claimed; the current guest fixture consumes ABI v10.

M5 retains trusted platform/requester and compatibility expansion. M6 retains
service integration, guest/physical execution and performance evidence. Staged
Flat Delete/Split/Merge, FaultRoute and the remaining External lifecycle gates
are still not activated by this change. The [M1 activation requirements](memory-m1.md#migration-order-and-acceptance-gates)
require coordinated provider, consumer and image verification; this delivery
does not change issue checkboxes, merge the stack or activate ABI v11.
