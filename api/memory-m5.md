# M5 trusted hardware admission delivery

This staged provider follows [M4](memory-m4.md), PR #151 at
`def3a80b3f8a43b357b6ecf4d995f64064bbcb6a`. It implements issue #142's
whole-group hardware admission and fail-closed unsupported boundary. Running
ABI/Ledger/Capsule remain **10**. The boot qualification entry refuses installation
until the coordinated next ABI is active; exporting it does not activate v11.

## Qualified platform boundary

Trusted integration constructs an immutable `memory::topology::Topology` and
passes it to `entry::boot::qualify_memory` after RAM admission. Its unsafe
constructor requires actual platform evidence, not firmware flags or a Unit's
coherent assertion. Each fabric describes complete normal, cacheable,
little-endian RAM intervals and the controller/requester routes that can share
them continuously with the admitted CPU. The same fabric must cover the entire
physical Page and every member. Two CPU/device fabric claims cannot be combined
to admit a CPU/device/device group.

Kernel cross-checks the complete intervals against admitted boot RAM. Actual
PTEs retain normal/default memory attributes. External endpoints come from the
exclusive RISC-V adapter's controller, requester and newly allocated immutable
binding epoch; they do not come from the syscall. Existing controller ownership,
Sv39/PAS/endianness validation and disabled ATS/PRI/bypass paths remain required.
A new binding gets a new epoch even when the controller/requester is reused.
CPU endpoints use the non-reusing Nest instance for its one CPU-domain lease
lifetime, independently of numeric ASID reuse.

The first concrete CPU drain path supports **one admitted Hart**. It orders data
before translation exposure and, after exact translation invalidation, executes
an independent `fence rw, rw` before reuse. SMP qualification returns Unsupported:
a remote SFENCE acknowledgement alone is not treated as a remote data-drain
proof. Executable qualified access is also Unsupported; fabric coherence and a
data fence do not establish every Hart's instruction visibility. Existing v10
CPU execution paths are not relabeled as new qualified evidence.

| Path | This delivery |
| --- | --- |
| Qualified normal RAM, one Hart, registered coherent requester group | Read/write admission through the complete membership check |
| Unknown topology/route, physical interval outside the qualified fabric | Unsupported before staged publication effects |
| Multiple fabrics that only cover subsets of the actual group | Unsupported; no pairwise inference |
| Unknown legacy alias mixed with a qualified group | Refused in either admission order |
| SMP, executable qualified access, MMIO, noncoherent handoff, ATS/PRI, bypass | Unsupported; no fallback proof |
| Physical board or a different emulated topology | Requires its own trusted qualification and evidence |

This qualification is neither a public memory mode nor an authorization bit.
Original source/Nest permissions, provenance, BlockRef, versions, complete Page
geometry and maintenance gates remain independent requirements.

## Page membership and publication

Each qualified Retention owns one non-reusing membership identity. Page stores the
participants until the exact receipt settles, including prepared, publishing,
retiring and failed responsibilities. Missing completion quarantines the Page
and retains its member. Unpublished cancellation removes only its own entry.
Legacy responsibilities remain in the existing Page counters. Any difference
between those counters and the qualified member count means unknown participants,
which refuse a new fabric proof. Legacy admission also refuses a qualified group;
it cannot silently become a CPU or device member.

The membership gate covers group construction, compatibility verification and
counter registration. Retirement removes the same entry under that gate. A
preflight supplies precise staged Unsupported errors; actual preparation repeats
the check, so a competing publication cannot enter on a stale snapshot. Capacity
and allocation failures leave the prior members/counters intact. There are at
most 64 qualified simultaneous members per Page; existing legacy counter limits
are unchanged and legacy preparation allocates no membership entries. The
membership revision never wraps and exhaustion closes new admission while still
allowing cleanup. Globally non-reusing receipt identities use generation 1;
Memo Protect/republication creates a new receipt rather than reusing its identity.

Only this verified group path bypasses the legacy CPU/External conflict matrix.
Qualified CPU and multiple External readers/writers can coexist without evicting
a peer. Protect prepares replacement/admission receipts through the same group
check. Maintenance still requires the exact single permitted physical block;
compatibility cannot bypass a later overlapping block.

Qualified External source use retains the actual live source Pend's retirement
responsibility instead of creating an exclusive data loan. This permits CPU and
other qualified device accesses to remain live. It does not grant rights: source
checks and the final Page registration still precede exposure. Narrow/Revoke
continue to discover the exact Memo/source relationship and cannot discard the
retirement hold before physical settlement. Unqualified/private legacy borrowing
keeps its existing exclusion rules.

## Retirement and failure

No compatibility result is a retirement proof. The existing exact CPU invalidation
batch and External Projection/Batch paths remain the settlement owners. The
RISC-V adapter still clears the selected PTE, completes the exact invalidation and
IOFENCE.C PR/PW batch, then requires the independent matching platform Quiescence
proof. Partial publication, wrong/late completion, timeout and driver death do
not remove a member or release its Page.

External context reuse still requires every payload settled, DDTP Off, exact
Binding quiescence and drained faults before the controller returns to the pool.
Tables and command/completion queues keep their own Retentions. The new group
proof does not certify auxiliary storage or replace platform draining with a CQ
word. The generic M1 evidence types remain separate obligations; this concrete
provider uses Group/Compatibility during admission and preserves the existing
exact backend completion owners rather than synthesizing new completion tokens.

## Verification and limits

- Root formatting and locked host commands: **424 passed, 14 hardware tests
  ignored**. Eight new production Page/Topology tests cover multiple writers,
  precise settlement, incompatible group races, allocation failure, unknown
  aliases/routes, capacity, overlap, unsupported execution and quarantine.
- RV64 debug and release builds with existing production warnings denied.
- Explicit `iommu_on_real_qemu_registers` hardware test: **passed**, including
  all 11 scenarios on QEMU **10.0.13**, RV64, one Hart, 128 MiB, M-mode/no firmware.
  The new `dma-coherent` scenario qualifies the actual DT-resolved EDU requester,
  retains a CPU writer during real IOMMU DMA, checks both directions of visibility,
  retires the device while the CPU receipt survives, and checks denied writes
  after retirement and physical reuse. Existing partial/abandoned/exit, wrong
  route and read-only scenarios continue to run.
- Existing real-Kernel CPU/MMU and IOMMU regression tests: **2 passed** with
  QEMU 10.0.13 and OpenSBI 1.9, including debug/release and existing multi-Hart
  CPU scenarios. These exercise the running v10 paths, not qualified SMP.
  Four stale guest Sign constructors were migrated to the existing Reservation
  field so the actual guest builds and runs against the M4 base.

The hardware guest imports production Page, Publication, adapter, Domain and
Engine code. Its boot-only topology store and bare M-mode CPU access are fixture
boundaries: this is not native v11 trap/ledger execution, an SMP proof or a
physical-board result. Source-retirement integration and native Protect are
RV64-compiled and still need M6 service/composition execution. Multiple device
writers have host lifecycle/group evidence; the new QEMU scenario covers one
EDU writer and one CPU writer, not arbitrary multi-device topology.

The guest uses the existing Vanllia fixture revision
`8127d85230176b2483669d2242d62ef33bd36f6c`. It directly depends on the same pinned
`platform-riscv` and existing `fdt` packages to resolve the new scenario's actual
DT route; the older fixture's scenario switch otherwise supplies requester 0.
No provider pin or production dependency changed. Commands, serial logs and ELF
hashes are generated under `server/tests/guest/iommu/target/evidence/`.
The native Kernel regression uses the existing integration-fixture pin
`eb37b0b0db299513274864cec23cee509a3cd3cc`, with only its external manifests
pointed at this checkout. Its evidence is under
`server/tests/guest/cell/target/evidence/{cpu,io}/`.

No PR merge, issue checkbox update or ABI activation is part of this delivery.
M6 retains native v11 service integration, activation gates, performance and
additional platform/SMP/executable-path qualification.

The [path follow-up](memory-paths.md) connects staged Flat Delete, partial
Split/Merge, Reservation fault routes and bounded continuations, and versioned
External Delete. It records the remaining native service verification boundary.
