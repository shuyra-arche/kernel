# Authorized memory activity sampling

Status: version 1 CPU sampling implementation for #139. Only explicitly
qualified hardware-updated 4 KiB CPU leaves are supported. External and larger
Cell/leaf geometry return Unsupported. The global statistics page still returns
UNKNOWN for A/D; no per-recipient facts are added to that page.

## Provider and architecture

The implementation extends the atomic PTE prerequisite
[#181](https://github.com/shuyra-arche/kernel/pull/181), the all-Hart data
completion backend [#182](https://github.com/shuyra-arche/kernel/pull/182),
and the M0 contract [#177](https://github.com/shuyra-arche/kernel/pull/177).
The original M0 checkpoint was `b2eb37e2af134e4bb4248df454c437b06c1f93c0`.

The RV64 backend selects Sv39/Sv48/Sv57 geometry. Normal publication presets
A/D. A sampler's first atomic clear discards those preset and pre-authorization
bits. Its Memo owns cumulative history and exact nonblocking fence progress;
receiver subscriptions own delivery cursors. Full-width CAS preserves all
non-A/D bits and refuses a replaced PTE. Existing split/replacement/retirement
still owns physical lifetime; sampling neither splits a leaf nor retains a
physical Page reference as a reclamation interlock.

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
| Qualified CPU hardware A/D update, selected paging mode and fence path | May arm a version 1 epoch for an actual 4 KiB leaf |
| Svade/software-managed A/D | Unsupported until fault handling explicitly maintains equivalent facts |
| Unknown ADUE/firmware/virtualization configuration | Unsupported; retain preset A/D and normal memory behavior |
| External/IOMMU/device writes | Unsupported in the first implementation |

The boot provider must explicitly qualify the capability and its Hart coverage.
It must not infer it from QEMU's name, the host architecture, a global memory
Share, or successful DMA command completion. Existing providers default to no
sampling capability. The unsafe `boot::qualify_activity` boot attestation is separate from data or
instruction completion qualification. Existing providers remain unqualified
until their boot owner explicitly attests these conditions.

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
| Inspect Memo metadata | INSPECT in the current reservation window | Existing descriptive Memo metadata; no A/D clearing or activity grant |
| Derive an activity View | INSPECT plus SAMPLE in the selected reservation, and the existing SHARE creation license | Receiver-local `(Sign, Share View)` Pends; INSPECT alone is Denied |
| Derive on an unsupported backend/geometry | Live INSPECT plus SAMPLE in the selected scope | Unsupported before grants or PTE changes |
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

Use the existing Kernel observation Task. The implementation bounds are 16 live
subscriptions system-wide, one page of output per subscription and at most
eight Memo steps per turn, with a persistent fair cursor. A step can submit or
poll one covering synchronization attempt; it cannot spin waiting for a Hart.
Cross-source traversal and allocator calls are outside global COMMIT. Full
subscription capacity returns Busy without evicting a reader. No turn walks all
page tables or aliases. The executable budget check and timing histograms are in the
[activity guest](../server/tests/target/basis/nest/memo/activity/README.md). A count bound does
not imply a hard time bound for locks, allocator work or emulator scheduling.

Output Share grants are read-only and scoped to the actual recipient. Sign
announces only that its snapshot changed; it is not a sampling or retirement
proof. The output must be tied to the original live authority for withdrawal,
including already mapped copies. The global Memory Share remains unchanged
and never contains the union of per-Unit samples.

## Public interface and compatibility review

API, Client and Server change together in one provider revision. This is an
extension of the source-pinned ABI 11 contract, not a mixed-revision binary
compatibility guarantee:

| Surface | Version 1 decision |
| --- | --- |
| Entry | Existing Share.Derive (class 10, slot 4); no added call or reopened retired memory slot |
| Request | Existing 104-byte AccordRequest: source Pend, Caller/Task recipient, descriptive Mark and ordinary readonly Share limits |
| Selection | Limits select one fixed-origin Pages coordinate; the live source must be a Reservation Pend with INSPECT and SAMPLE |
| Authority | Reservation-only `NestRights::SAMPLE = 8192`; the sender authorizes sampling, while the recipient needs only the delivered readonly View and notification |
| Result | Existing call returns the receiver-local Share Pend; its companion Sign is published atomically in the same recipient ledger, marked with that Share Pend ID |
| Pair | `(Sign Pend, Share View Pend)` in that order; API validates the Kernel-origin metadata, Client resolves it using existing Pend.Inspect/Collect |
| View | One actual 4096-byte page at the selected Reservation coordinate; inaccessible sparse holes contain no other samples; map the View Pend's actual page window |
| Sample | Protocol 1, 128 checked bytes in a 4096-byte seqlock page; allocation/counters stop on exhaustion |
| Period/capacity | Initial fixed 10 ms policy, 16 admitted/pending subscriptions, eight slot visits per turn; actual intervals include fair scheduling and fence completion |
| Cancel/ack | Return either output Pend to stop delivery; consume the exact Sign receipt only after a coherent read; return remaining dead registrations to release their ledger charges |

No operation number, request width, Capsule format (2), Ledger version (11) or
global memory-page layout changes. No new authorization domain, Swap call,
opaque subscription identity or decorative operation pair is introduced.
Share.Derive keeps its ordinary Flat/Share behavior; only a qualified Reservation
source with explicit sampling authority produces activity data. Unsupported is
an added Share failure vocabulary, not a new call. Period aggregation and
candidate ranking belong to the consumer.

The pair's Mark is descriptive correlation; it never supplies sampling
permission. Live sender/recipient, source rights/window and exact publication
identity are revalidated before the two grants are published in one COMMIT.
The recipient cannot trigger the Sign, write the View or redistribute either
Pend. The source sender can withdraw its original authority independently of
the recipient; sender exit also stops delivery. Both output registrations
charge the real requester's ordinary incoming/per-Task/per-Team admission
budgets, even though their producer/owner is Kernel. Preparing the second
registration can fail without publishing the first or leaking its charge. Returning either output cancels
the stream while the original source grant keeps its ordinary ownership.

Busy before Share.Derive returns has admitted no pair. Once the View is returned,
keep that same View if a bounded Client pair lookup needs retrying. Do not issue
a second Derive to recover a lookup or lost notification. The existing source
and output Pends own authorization and cancellation; Mark and Page IDs do not.

Old strict PendInfo/Limits decoders do not know bit 8192 and may reject a newly
issued Reservation. Upgrade API, Client, Server, consumer binaries and image
producers to one matching full source SHA and rebuild the complete image.
Do not combine an old ABI 11 rights decoder with this Server. Rollback requires
rebuilding the complete old pinned image. Existing release tags remain
immutable and do not acquire sampling. The sample page has its own explicit protocol version; an unknown page version
does not decode as a completed sample.

## Progress, cancellation and validity

`SamplePage::read() == None` means no completed coherent sample, including first
arm and an unfinished fence; Sign remains Pending. It does not mean false A/D.
Observed implies a qualified CPU leaf and one complete interval after the
reader's fresh baseline. `start_ns` and `end_ns` label the preceding and current
atomic clears; `synchronized_ns` is the covering completion after the current
clear. The interval between either clear and its covering synchronization can
include old translations or speculative walks. NotObserved is only absence of
captured evidence for that declared uncertainty, never a safe-clean proof.
Other readers may cause intermediate clears while a recipient accumulates an
interval; their clear/fence uncertainty also applies. The last synchronization
timestamp is not a continuous-write coverage guarantee.

The Memo stores the last completed epoch containing each true bit. A reader's
cursor advances only on its own exact Sign acknowledgement. Unread output is
not rewritten; another reader can keep sampling without erasing its facts.
A replacement yields one terminal Stale sample with the original identity and
Unknown evidence. The old subscription does not follow the replacement.

Narrow, Revoke, Return and sender/recipient Task/Team exit invalidate output grants before future
writes. Existing SourceAuthorization and Nest retirement retract actual output
mappings. Cancellation processes one native binding and one nonblocking fence
post/poll per visit; the registry slot and output storage remain retained until
withdrawal and the captured sampling round both settle. The private output
Flat closes through the same physical-return checks; retaining dead Pend
metadata does not keep its Page allocation alive. Peers can finish the
same parked Nest retirement; they cannot substitute another request.

Stopping the observation Task can retain at most the admitted subscription
storage/progress and fill this service's capacity. It does not retain observed
physical Pages, authorize new writes, or prevent ordinary Retract/Block/Vacate,
allocation or explicit output Return from completing through existing owners.
The stopped-task QEMU scenario exits the real Kernel Task immediately after its
first captured clear, then checks those operations without restarting it.

## Verification boundary

The [test guide](../server/tests/target/basis/nest/memo/activity/README.md) records reproducible
host, RV64 and QEMU checks, test-only instrumentation and fault injection.
Evidence records exact source/fixture/toolchain, CPU configuration, firmware,
composition diff, lockfile and ELF hashes, per-turn counts, timing histograms
and raw writer throughput. The restricted consumer sorts two authorized
candidates itself; the Kernel implements no eviction or writeback policy.

The tested emulator configuration uses Sv57 and QEMU's legacy hardware A/D
mode with Svade/Svadu explicitly disabled. Unqualified Svade is a separate
negative case. Sv39/Sv48, ADUE-enabled Svadu firmware, physical hardware,
hotplug and External activity have no sampling execution evidence here.
Neither QEMU maxima nor CPU D bits establish real-hardware WCET, device-write
absence, persistence, or permission to reclaim a Page.

## Resident work entry convention

[`entry/work`](../server/src/entry/work/mod.rs) groups real independently
scheduled Kernel Team Tasks. Each child keeps its Task construction/start,
entry loop, wake source and budget together; boot explicitly orders startup.
The observer's `start`, `run`, deadline sleep and resource notification remain
in `work/observe`, with the same snapshot Page, read-only Share and update Sign.
It uses the existing trusted breakpoint continuation path, not S-mode `ecall`.
No borrowed resource guard survives its wait.

Lifecycle now also has an independent resident Task; retirement remains separately owned:

- Unit's `lifecycle/reap::advance` services up to four pending terminal Tasks and
  four retained husks per turn, retains Busy retirement and arms deadline retry.
  `entry/work/lifecycle` runs that budget on its own Task, with an independently
  reserved Sign Site. Runnable local stages yield; retained Busy obligations arm
  a 1 ms retry and sleep. Ordinary scheduler/idle paths poll due notifications.
  They retain cleanup during startup, shutdown or a 100 ms service watchdog.
  The Task closes its registration before Exit; scheduler-stack fallback then
  reaps that Task too. An unexpectedly dead service is unregistered by its owner.
  No task is excluded from `tally::done()` accounting.
- Nest's `authority/external::maintain` combines Cell changes, retained binding
  progression and interrupt-router rearming. The latter has controller-Hart
  locality. There is no separately registered retirement notification/Task
  lifetime here; moving this whole call to a migratable Task would violate the
  locality boundary. Retained translation obligations stay with Nest, and
  synchronous work remains synchronous.

Observer and lifecycle are instantiated under `work` today. Retirement, memory
and interrupt directories remain absent. Nest retirement still requires a bounded
owner step and event admission before an independent Task is justified: binding
maintenance currently scans organization records, and Page completion wakeup
traverses the source index. Moving those loops alone would hide the existing
bounds/notification gap behind a Task. Controller rearming remains on its existing
valid Hart paths.

A future
independent Task must first supply an owner-controlled bounded step, distinguish
runnable remainder from hardware-blocked obligations, register its own wake
source before sleeping, and preserve scheduler-stack shutdown/failure progress.
The observer is not a dispatcher for those responsibilities.

The lifecycle wait uses a private selector authenticated against its registered
Kernel Task identity, after the same instruction validation and saved-PC advance
as ordinary calls. It is not a public ABI operation or S-mode ecall. The reserved
Site retains early/coalesced wakes under COMMIT. Enrollment uses a reserved ticket
with no timer/holder allocation and debug enforces its allocation-free section.
All identity borrows and guards end before scheduler-stack handoff.
