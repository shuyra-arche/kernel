# Authorized memory activity sampling

Status: M0 proposal for #139. This document does not enable page sampling or
claim completion of M1–M3. The current implementation still reports UNKNOWN in
the global memory snapshot. Public operation numbers and rights are not issued
by this proposal; they require the API/Client/Server compatibility review below.

## Current provider and architecture

The source checkpoint is Kernel `b2eb37e2af134e4bb4248df454c437b06c1f93c0`,
with ABI 11 as its sole dispatch contract. The Kernel Team observation Task is
in `server/src/entry/observe/`; its global Share/Sign page has no per-page data.
The earlier #138 implementation is present, but its historical guest fixture
is not evidence of current ABI 11 acceptance.

The RV64 backend selects Sv39/Sv48/Sv57 geometry. `Nest::attrs` currently
presets both ACCESSED and DIRTY when publishing CPU PTEs. Those bits therefore
cannot establish that a guest accessed or wrote a page. `PageTableEntry`
currently stores ordinary u64 values; `set_flags` performs a load/modify/store.
Enabling hardware changes to A/D requires atomic full-width PTE access and an
audit of permission changes, splitting, replacement and retirement, not just
an A/D getter. Neither the paging mode nor an ISA extension name alone proves
that hardware updating is enabled for this execution environment.

Architecture reference: RISC-V Instruction Set Manual, Volume II, release
20250508, source
[`ed0aaa296203a68f7b1f4a231c3971cd3ed28973`](https://github.com/riscv/riscv-isa-manual/tree/ed0aaa296203a68f7b1f4a231c3971cd3ed28973).
The applicable sections are
[`supervisor.adoc`, Sv32 translation and Svadu 1.0](https://github.com/riscv/riscv-isa-manual/blob/ed0aaa296203a68f7b1f4a231c3971cd3ed28973/src/supervisor.adoc)
and
[`machine.adoc`, menvcfg.ADUE](https://github.com/riscv/riscv-isa-manual/blob/ed0aaa296203a68f7b1f4a231c3971cd3ed28973/src/machine.adoc).
The same translation A/D rules apply to the selected RV64 modes. A may be set
speculatively. D updates for explicit stores are exact, but neither bit proves
completion of a larger operation. Hardware never clears them. Svade faults
when a required bit is clear; Svadu's ADUE control determines which behavior
applies when that extension is present.

| Backend capability | Initial behavior |
| --- | --- |
| Qualified CPU hardware A/D update, selected paging mode and fence path | May arm a sampling epoch after full-width atomic PTE support and exact cross-Hart synchronization are verified |
| Svade/software-managed A/D | Unsupported until fault handling explicitly maintains equivalent facts |
| Unknown ADUE/firmware/virtualization configuration | Unsupported; retain preset A/D and normal memory behavior |
| External/IOMMU/device writes | Unsupported in the first implementation |

The boot provider must explicitly qualify the capability and its Hart coverage.
It must not infer it from QEMU's name, the host architecture, a global memory
Share, or successful DMA command completion. Existing providers default to no
sampling capability. No current boot attestation grants this capability.

## Observation and authority

The observation object is one Memo's actual CPU PTE leaf, not Page-global
activity. Its identity contains NestId, reservation identity, Memo coordinate,
MemoGeneration, PageId and effective leaf geometry. The PageId correlates a
sample with an allocation; it confers no authority and does not cause an alias
search. FlatId, MemoId, PageId, Team membership and the global statistics Share
cannot register a sampling range.

Every operation resolves a caller-local live Pend again, using its current
Nest reservation and narrowed window. A leaf must lie completely inside that
window. A large leaf crossing its boundary is Unsupported; sampling must not
split a mapping merely to observe a restricted window. No result aggregates
other Memos, other Nests or unauthorised aliases of the same Page.

| Operation | Required authority | Result or failure |
| --- | --- | --- |
| Read an already available Memo sample | INSPECT in the current reservation window | Descriptive sample, Busy if incomplete, Stale on identity change |
| Register or rearm a period that clears A/D | INSPECT plus a separate, explicitly delegated sampling right | Bounded subscription and receiver-local Share/Sign; INSPECT alone is Denied |
| Inspect backend capability | Live INSPECT in the selected scope | Qualified capability or Unsupported; no Page facts outside the window |
| Cancel | Actual subscription owner, or loss of any required live authority | Stop delivery and retract its output mappings; repeat cancellation is harmless |

Read-only data access and the sampling right are separate from permissions to
write the observed memory. Narrowing may remove either observation authority
or the sampling right; a copied registration must not retain the old rights.
The existing Pend/Source/Memo owners remain authoritative. A subscription holds
references and progress, not a new mapping or authorization directory.

## Epochs, atomicity and multiple readers

A single sampling owner per Memo clears its PTE; observers never clear each
other's facts. Memo-owned monotonic accumulated access/write evidence survives
publication to any particular observer. Each registered reader tracks its own
delivered cursor. Registration starts with Unknown and advances its baseline
at a subsequent completed epoch boundary, so facts from before authorization
are not exposed to a newly registered reader.

1. Under the Nest's existing operation/state protection, verify the exact live
   Memo, generation, Page identity, leaf extent and capability. Acquire progress
   ownership before any PTE change. Do not hold global COMMIT through a fence.
2. Atomically read and clear only A/D in the complete aligned XLEN-sized PTE.
   Use a checked CAS/fetch operation; permission changes must preserve concurrent
   hardware A/D updates. Capture the old facts before starting synchronization.
3. Release software locks, retain the exact operation/generation and invoke the
   covering local and remote translation synchronization. Global leaves require
   the global scope. A later request cannot consume an earlier fence's proof.
4. On exact completion, commit the next epoch's starting boundary and accumulated
   evidence under the same owner. Failed/partial synchronization retains its
   captured facts and retry responsibility, with no successful next epoch.
5. Publish an observer's coherent snapshot before its Sign update. Until that
   exact version is acknowledged, either retain the snapshot or conservatively
   accumulate undelivered facts. Do not overwrite an unread true with false.

The boundary between atomic clearing and completed remote synchronization is
uncertain: old translations and speculative walks can overlap it. A false bit
means only NotObserved for the declared window and uncertainty, never globally
clean or unused. Captured true evidence is retained across Busy, timeout and
retry. Counter/epoch exhaustion stops sampling explicitly; it never wraps into
a plausible fresh sample. Readers resynchronize after a lost/expired cursor
and receive Unknown until a complete new interval is available.

## Sample and delivery contract

The per-recipient protocol starts at version 1, independent of the unchanged
global statistics page version. A sample carries protocol version, monotonically
increasing delivery sequence, epoch, start/end timestamps, object identities
and generation, leaf base/byte extent, access and write evidence, capability,
and validity/uncertainty. Unknown, Unsupported, Pending, Stale and Observed are
distinct. Unknown must never decode as false. Page-global cleanliness and
writeback completion are deliberately unrepresentable.

| Lifecycle event | Sampling behavior |
| --- | --- |
| Register | Allocate all bounded storage before admission, revalidate authority before publishing output grants; OOM publishes nothing |
| First arm | Unknown until a fresh, fully synchronized interval starts after registration |
| Partial batch/fence failure/timeout | Pending with exact cursor and captured facts retained; retry does not clear again |
| Insert/Remove/Publish/Retract or association replacement | Reject the old MemoGeneration/PageId; never attach its facts to the replacement |
| Push/Pull or Split/Merge | Existing physical maintenance rules apply; sampling cannot authorize relocation, pinning or reuse |
| Narrow/Revoke/Return, Task/Team exit | Stop future delivery, retract output grants/mappings through existing retirement; discard unauthorized pending output |
| Consumer or observation Task stops | No effect on allocation, faults, mapping retirement or Page reclamation responsibility |
| Cancel during an unfinished fence | Stop delivery immediately; keep backend progress until its exact synchronization responsibility settles |

Use the existing Kernel observation Task. Proposed initial bounds are 16 live
subscriptions system-wide, one page of output per subscription and at most
eight Memo steps per turn, with a persistent fair cursor. A step can submit or
poll one covering synchronization attempt; it cannot spin waiting for a Hart.
Cross-source traversal and allocator calls are outside global COMMIT. Full
subscription capacity returns Busy without evicting a reader. No turn walks all
page tables or aliases. These bounds need measured verification before release.

Output Share grants are read-only and scoped to the actual recipient. Sign
announces only that its snapshot changed; it is not a sampling or retirement
proof. The output must be tied to the original live authority for withdrawal,
including already mapped copies. The global Memory Share remains unchanged
and never contains the union of per-Unit samples.

## Compatibility and implementation gates

Before assigning calls, review the smallest request/result form, new sampling
right, cancellation/acknowledgement rule, descriptor widths, codec validation,
protocol negotiation and old-client behavior together across API, Client and
Server. No Swap-specific call or independent resource domain is needed. Paired
public names, if any are necessary, must meet the equal-length and semantic
duality convention. The current ABI 11 global statistics contract remains
explainable; an incompatible wire change requires a coordinated ABI migration.

M1 requires atomic PTE access across every mutator, backend qualification,
checked leaf geometry and retained exact fence progress. M2 requires grant
withdrawal through real mapping retirement, fair budgeted Task execution and
receiver-local Share/Sign. M3 requires current-ABI host contract tests, actual
RV64 debug/release builds, a restricted consumer, and QEMU tests for reads,
writes, idle periods, multiple readers, alias isolation, stale identity,
partial synchronization and revocation. Multi-Hart continuous writers and
unsupported configurations require separate evidence. The current native
composition's one-Hart qualification cannot stand in for those scenarios.

Record exact source/fixture/toolchain, firmware capability settings, commands,
failures, distributions, per-turn step counts, fence attempts and load impact.
Neither QEMU maxima nor CPU D bits establish real-hardware WCET, device-write
absence, persistence, or permission to reclaim a Page.
