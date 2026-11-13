# Native acceptance and coordinated version selection

This follow-up covers #142 §14.8 and the public construction boundary in #159.
The M1–M6 records describe their original delivery snapshots; the additional
native scenarios live in Vanllia `tests/memory/guest/acceptance` and `virtqueue.rs`.
They use real Task ledgers and public calls, including independent Teams and an
actual QEMU virtio-blk requester behind the IOMMU. Host models alone do not prove
these execution paths.

## Source derivation and maintenance continuation

Native ABI 11 adds Share class 10, slot 4, `Derive { license, request }`.
`request` addresses one 104-byte `AccordRequest`, copied once before use. The
source is a live Flat or Share; limits select Share operations and a page window.
The caller must hold a None/SHARE creation license. A Flat additionally requires
SHARE and the corresponding ASSOCIATE/DELETE source rights; write/distribution
ceilings and actual ancestry remain enforced by the existing Pend transaction.
The recipient is Caller or an authorized adjacent Task. No Cell reference or
physical ownership is created. v10 rejects slot 4; v11 still rejects old Share
Create/Open/Shut and Cell adapters. Ordinary Accord does not change target kinds.

`Block { recover: Some(reference), ... }` has two explicit execution paths:

- The same live Task and exact authorization branch resume their Draining or
  Held record, with identical Flat/range/epoch. This can settle a retained Busy
  result after device reset without creating another blocker.
- An orphaned record requires a real ancestor covering its original range;
  retirement precedes takeover, and only one caller can advance the epoch.

Neither path treats a credential number as permission. Releasing/Closed records,
wrong branches, wrong ranges, stale epochs and competing takeovers reject.
Busy after admission retains the existing record and physical responsibilities;
the caller uses a new request ID for explicit progress, while exact replay still
returns the original journal result.

## Public User Team construction (#159)

`UnitCreate::Team` no longer has a `kind` field, and public `ProgramKind` is
removed. Its 48-byte descriptor remains `[0, 0, guard, nest, member_exit, 0]`.
The second word is reserved zero: legacy Supervisor `1` and every other nonzero
value reject during snapshot decoding, before allocation/publication. They are
never normalized to User. Both supported ABI selections apply this restriction;
valid existing User bytes retain their meaning. This is a Rust source break and
a deliberate removal of the formerly accepted privileged wire subset.

The entry constructs only `SpaceKind::User`, retaining Build, guard, member-exit,
creator, exclusive empty Nest adoption, ATTACH consumption and Task control
checks. Both legacy and native public Nest creation already select User; adoption
rejects Supervisor/external Nests. Tasks continue to belong to an existing Team
under the existing lifecycle rules; Task creation cannot manufacture a new Team.
Internal Capsule/bootstrap construction still selects Supervisor, and Kernel
Team setup and hardware privilege mechanisms remain internal and unchanged.
This does not alter the ebreak/ecall transport or assume the observer is U-mode.

Vanllia's direct current consumer is updated after this provider. Historical
System/env and device fixtures pinned to older APIs are separate migrations;
changing their labels or pins alone would not migrate their legacy operations.

## Validation boundary

Vanllia selects `memory-v11` consistently for API/Client/Server, the normal RV64
platform, QEMU platform fixture and Capsule packager. All use one published
provider revision with regenerated lockfiles; Ledger and Capsule ABI derive from
the selected API version. Kernel retains its explicitly documented v10 default
for legacy regression consumers. User-requested product scope is version
unification, not rewriting the legacy System/service operation graph.

Acceptance requires debug/release native runs, rejected ABI 10 Capsule, portable
contract checks and RV64 builds with warnings denied. The runner preserves each
image, firmware/lockfile hashes, command, serial output and exit status. Tests
must reach final IOMMU shutdown, not merely print intermediate PASS markers.
No physical machine is available; physical validation remains not run. QEMU's
reviewed reset/drain evidence does not qualify unknown hardware or SMP.
