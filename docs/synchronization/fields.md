# Current lock fields and acquisition relations

Generated with `python3 server/tests/sync/inventory.py`. The [audit](README.md) supplies owner, lifetime, failure, IRQ and migration obligations. The [complete call-site catalogue](acquisitions.json) links each textual acquisition to its source function; overloaded names and `with` calls require the source context. This is a review index, not execution coverage.

Only this table’s predecessors/successors are a display of the transitive reduction of the production `relations!` closure. They are permitted directions, not claims that every possible edge is executed. All omitted ordinary combinations are forbidden; controlled peers and source-borrow try admission are separate protocols.

| Class | Current typed storage/navigation | Minimal declared predecessors | Minimal declared successors |
| --- | --- | --- | --- |
| `ProbeRegistry` | injected guest probe only |  |  |
| `LiveChanges` | [basis/cell/change.rs:9](../../server/src/basis/cell/change.rs#L9) | `Flat` | `ChangeBlocks` |
| `PageMembership` | [basis/cell/page/mod.rs:61](../../server/src/basis/cell/page/mod.rs#L61) | `ChangeBlocks`, `Controllers` | `BlockPool`, `BlockPump`, `RetentionStatistics` |
| `ChangeBlocks` | [basis/cell/page/reclaim/change.rs:29](../../server/src/basis/cell/page/reclaim/change.rs#L29) | `LiveChanges` | `PageMembership` |
| `RetentionStatistics` | [basis/cell/page/reclaim/statistics.rs:11](../../server/src/basis/cell/page/reclaim/statistics.rs#L11) | `PageMembership` |  |
| `Commit` | [metal/riscv/sync/lock/mod.rs:45](../../server/src/metal/riscv/sync/lock/mod.rs#L45) | `LedgerSerial`, `BootRegions` | `ActivityDelivery`, `Address`, `BlockStages`, `BlockUses`, `ControlTable`, `Deadline`, `HeldSources`, `Husks`, `LedgerHeirs`, `Organizations`, `PendParent`, `ProvenanceGrant`, `ProvenanceUses`, `Scheduler`, `SchedulerAdmission`, `SourceAuthorities`, `SourceAuthorizationRef`, `SourceBindings`, `SourceGate`, `SourceMaps`, `SourceOrigins`, `SpanExports`, `TaskResult`, `TeamChildren`, `TeamExit`, `TeamTasks`, `TerminationQueue`, `WaitHolders` |
| `Flat` | [basis/flat/mod.rs:58](../../server/src/basis/flat/mod.rs#L58), [basis/flat/mod.rs:136](../../server/src/basis/flat/mod.rs#L136), [basis/flat/storage/materialize.rs:32](../../server/src/basis/flat/storage/materialize.rs#L32), [basis/flat/storage/materialize.rs:41](../../server/src/basis/flat/storage/materialize.rs#L41) | `Reservation`, `BlockPages` | `LiveChanges`, `SourceIndex` |
| `SourceAuthorizationRef` | [basis/flat/sharing/mod.rs:52](../../server/src/basis/flat/sharing/mod.rs#L52) | `Commit` |  |
| `SourceMaps` | [basis/flat/sharing/mod.rs:56](../../server/src/basis/flat/sharing/mod.rs#L56) | `Commit` |  |
| `SourceAuthorities` | [basis/flat/sharing/mod.rs:57](../../server/src/basis/flat/sharing/mod.rs#L57) | `Commit` | `BlockPool`, `BlockPump` |
| `ActivityRegistry` | [basis/nest/activity.rs:51](../../server/src/basis/nest/activity.rs#L51) | `SourceGate` |  |
| `ActivityDelivery` | [basis/nest/activity.rs:104](../../server/src/basis/nest/activity.rs#L104) | `Commit` | `SignState`, `WaitSites` |
| `FaultPolicy` | [basis/nest/address/span/mod.rs:31](../../server/src/basis/nest/address/span/mod.rs#L31) | `Reservation` | `BlockPool`, `BlockPump`, `FaultProgress`, `SignQueue`, `SignState` |
| `SpanExports` | [basis/nest/address/span/mod.rs:41](../../server/src/basis/nest/address/span/mod.rs#L41) | `Commit` | `BlockPool`, `BlockPump` |
| `Reservation` | [basis/nest/address/span/mod.rs:36](../../server/src/basis/nest/address/span/mod.rs#L36) | `Nest` | `FaultPolicy`, `Flat` |
| `Organizations` | [basis/nest/authority/binding.rs:35](../../server/src/basis/nest/authority/binding.rs#L35) | `Commit` | `Nest` |
| `NestAuthority` | [basis/nest/authority/mod.rs:105](../../server/src/basis/nest/authority/mod.rs#L105) | `Nest` | `BlockPool`, `BlockPump` |
| `FaultProgress` | [basis/nest/fault/queue.rs:13](../../server/src/basis/nest/fault/queue.rs#L13) | `FaultPolicy` |  |
| `SourceIndex` | [basis/nest/index/source.rs:37](../../server/src/basis/nest/index/source.rs#L37) | `Flat` | `BlockPool`, `BlockPump` |
| `ActivityHistory` | [basis/nest/memo/activity/mod.rs:37](../../server/src/basis/nest/memo/activity/mod.rs#L37) | `Nest` |  |
| `Address` | [basis/nest/mod.rs:100](../../server/src/basis/nest/mod.rs#L100) | `Commit` | `Nest` |
| `Cleanup` | [basis/nest/mod.rs:101](../../server/src/basis/nest/mod.rs#L101) |  | `Nest` |
| `Nest` | [basis/nest/mod.rs:102](../../server/src/basis/nest/mod.rs#L102) | `Organizations`, `Address`, `Cleanup`, `BootNests` | `ActivityHistory`, `Controllers`, `NestAuthority`, `Reservation`, `SourcePhase`, `TaskJournal` |
| `PendState` | [basis/pend/authority/mod.rs:32](../../server/src/basis/pend/authority/mod.rs#L32) | `LedgerEntries`, `SourcePhase`, `ProvenanceGrant` |  |
| `PendParent` | [basis/pend/authority/mod.rs:33](../../server/src/basis/pend/authority/mod.rs#L33) | `Commit` |  |
| `LedgerAdmission` | [basis/pend/ledger/admission.rs:29](../../server/src/basis/pend/ledger/admission.rs#L29) | `LedgerCharge` | `BlockPool`, `BlockPump` |
| `LedgerCharge` | [basis/pend/ledger/entry.rs:31](../../server/src/basis/pend/ledger/entry.rs#L31) | `LedgerEntries` | `LedgerAdmission` |
| `LedgerSerial` | [basis/pend/ledger/table.rs:25](../../server/src/basis/pend/ledger/table.rs#L25), [metal/riscv/sync/lock/ledger.rs:21](../../server/src/metal/riscv/sync/lock/ledger.rs#L21), [metal/riscv/sync/lock/ledger.rs:23](../../server/src/metal/riscv/sync/lock/ledger.rs#L23), [metal/riscv/sync/lock/ledger.rs:63](../../server/src/metal/riscv/sync/lock/ledger.rs#L63) |  | `Asid`, `Commit` |
| `LedgerEntries` | [basis/pend/ledger/table.rs:29](../../server/src/basis/pend/ledger/table.rs#L29) | `BlockEpoch` | `CloneQueue`, `LedgerCharge`, `PendState` |
| `LedgerHeirs` | [basis/pend/ledger/table.rs:30](../../server/src/basis/pend/ledger/table.rs#L30) | `Commit` | `BlockPool`, `BlockPump` |
| `BlockPages` | [basis/pend/source/admission.rs:12](../../server/src/basis/pend/source/admission.rs#L12) | `BlockEpoch`, `BlockStages` | `Flat` |
| `BlockEpoch` | [basis/pend/source/admission.rs:13](../../server/src/basis/pend/source/admission.rs#L13) | `SourceBlocks` | `BlockPages`, `LedgerEntries`, `TaskState`, `TeamState` |
| `BlockUses` | [basis/pend/source/admission.rs:14](../../server/src/basis/pend/source/admission.rs#L14) | `Commit` | `BlockPool`, `BlockPump` |
| `BlockStages` | [basis/pend/source/admission.rs:15](../../server/src/basis/pend/source/admission.rs#L15) | `Commit` | `BlockPages` |
| `Holds` | [basis/pend/source/hold.rs:136](../../server/src/basis/pend/source/hold.rs#L136) |  |  |
| `HeldSources` | [basis/pend/source/hold.rs:137](../../server/src/basis/pend/source/hold.rs#L137) | `Commit` | `BlockPool`, `BlockPump` |
| `SourceFlights` | [basis/pend/source/mod.rs:79](../../server/src/basis/pend/source/mod.rs#L79) | `SourceGate` | `BlockPool`, `BlockPump` |
| `SourceGate` | [basis/pend/source/mod.rs:81](../../server/src/basis/pend/source/mod.rs#L81) | `Commit` | `ActivityRegistry`, `InterruptProgress`, `InterruptSource`, `InterruptSources`, `SignRouting`, `SignState`, `SourceBlocks`, `SourceFlights`, `SourcePhase`, `WaitSites` |
| `SourceBindings` | [basis/pend/source/mod.rs:87](../../server/src/basis/pend/source/mod.rs#L87) | `Commit` | `BlockPool`, `BlockPump` |
| `SourceBlocks` | [basis/pend/source/mod.rs:88](../../server/src/basis/pend/source/mod.rs#L88) | `SourceGate` | `BlockEpoch` |
| `SourceOrigins` | [basis/pend/source/mod.rs:89](../../server/src/basis/pend/source/mod.rs#L89) | `Commit` | `BlockPool`, `BlockPump` |
| `SourcePhase` | [basis/pend/source/mod.rs:90](../../server/src/basis/pend/source/mod.rs#L90) | `Nest`, `SourceGate` | `PendState` |
| `ProvenanceGrant` | [basis/pend/source/provenance.rs:11](../../server/src/basis/pend/source/provenance.rs#L11) | `Commit` | `PendState` |
| `ProvenanceUses` | [basis/pend/source/provenance.rs:17](../../server/src/basis/pend/source/provenance.rs#L17) | `Commit` | `BlockPool`, `BlockPump` |
| `Deadline` | [basis/room/deadline/mod.rs:40](../../server/src/basis/room/deadline/mod.rs#L40), [basis/room/deadline/mod.rs:41](../../server/src/basis/room/deadline/mod.rs#L41) | `Commit` | `BlockPool`, `BlockPump` |
| `InterruptController` | [basis/room/interrupt/mod.rs:92](../../server/src/basis/room/interrupt/mod.rs#L92) | `InterruptSources` |  |
| `InterruptProgress` | [basis/room/interrupt/mod.rs:93](../../server/src/basis/room/interrupt/mod.rs#L93) | `SourceGate` |  |
| `InterruptSources` | [basis/room/interrupt/mod.rs:105](../../server/src/basis/room/interrupt/mod.rs#L105) | `SourceGate` | `BlockPool`, `BlockPump`, `InterruptController` |
| `InterruptSource` | [basis/room/interrupt/source/mod.rs:33](../../server/src/basis/room/interrupt/source/mod.rs#L33) | `SourceGate` |  |
| `Scheduler` | [basis/room/scheduler/slot/mod.rs:36](../../server/src/basis/room/scheduler/slot/mod.rs#L36) | `Commit` | `TaskExtensions`, `TaskSchedule`, `TaskState`, `TeamState` |
| `SchedulerAdmission` | [basis/room/scheduler/slot/mod.rs:39](../../server/src/basis/room/scheduler/slot/mod.rs#L39) | `Commit` | `TaskSchedule`, `TaskState` |
| `Shutdown` | [basis/room/shutdown/mod.rs:22](../../server/src/basis/room/shutdown/mod.rs#L22) |  |  |
| `SignQueue` | [basis/sign/endpoint/delivery.rs:13](../../server/src/basis/sign/endpoint/delivery.rs#L13) | `FaultPolicy` | `SignDelivery` |
| `SignState` | [basis/sign/endpoint/mod.rs:29](../../server/src/basis/sign/endpoint/mod.rs#L29) | `ActivityDelivery`, `FaultPolicy`, `SourceGate` |  |
| `SignRouting` | [basis/sign/endpoint/mod.rs:32](../../server/src/basis/sign/endpoint/mod.rs#L32) | `SourceGate` | `BlockPool`, `BlockPump` |
| `SignDelivery` | [basis/sign/endpoint/mod.rs:35](../../server/src/basis/sign/endpoint/mod.rs#L35) | `SignQueue` |  |
| `WaitHolders` | [basis/sign/wait/holder.rs:30](../../server/src/basis/sign/wait/holder.rs#L30) | `Commit` | `BlockPool`, `BlockPump` |
| `WaitSites` | [basis/sign/wait/site.rs:120](../../server/src/basis/sign/wait/site.rs#L120) | `ActivityDelivery`, `SourceGate` | `BlockPool`, `BlockPump`, `TaskState` |
| `ControlTable` | [basis/unit/control/table.rs:36](../../server/src/basis/unit/control/table.rs#L36) | `Commit` | `BlockPool`, `BlockPump` |
| `LifecycleDiagnostics` | [basis/unit/lifecycle/diagnostics/mod.rs:12](../../server/src/basis/unit/lifecycle/diagnostics/mod.rs#L12) |  |  |
| `TerminationQueue` | [basis/unit/lifecycle/doom.rs:83](../../server/src/basis/unit/lifecycle/doom.rs#L83) | `Commit` | `TeamState`, `TeamTermination` |
| `ReapQueue` | [basis/unit/lifecycle/reap.rs:24](../../server/src/basis/unit/lifecycle/reap.rs#L24) |  | `TaskState` |
| `Husks` | [basis/unit/lifecycle/reap.rs:26](../../server/src/basis/unit/lifecycle/reap.rs#L26) | `Commit` | `TaskState` |
| `TaskState` | [basis/unit/task/creation/prepared.rs:23](../../server/src/basis/unit/task/creation/prepared.rs#L23), [basis/unit/task/creation/prepared.rs:31](../../server/src/basis/unit/task/creation/prepared.rs#L31), [basis/unit/task/mod.rs:50](../../server/src/basis/unit/task/mod.rs#L50), [basis/unit/team/membership.rs:60](../../server/src/basis/unit/team/membership.rs#L60) | `BlockEpoch`, `Scheduler`, `SchedulerAdmission`, `WaitSites`, `ReapQueue`, `Husks`, `TeamTasks` |  |
| `TaskReceipts` | [basis/unit/team/membership.rs:62](../../server/src/basis/unit/team/membership.rs#L62) | `TeamTasks` | `BlockPool`, `BlockPump` |
| `TaskResult` | [basis/unit/team/membership.rs:64](../../server/src/basis/unit/team/membership.rs#L64) | `Commit` |  |
| `TaskJournal` | [basis/unit/task/mod.rs:53](../../server/src/basis/unit/task/mod.rs#L53) | `Nest` | `Frame` |
| `TaskHeir` | [basis/unit/task/mod.rs:58](../../server/src/basis/unit/task/mod.rs#L58) | `TeamChildren` | `BlockPool`, `BlockPump` |
| `TaskRetirement` | [basis/unit/task/mod.rs:63](../../server/src/basis/unit/task/mod.rs#L63) |  |  |
| `TaskSchedule` | [basis/unit/task/mod.rs:64](../../server/src/basis/unit/task/mod.rs#L64) | `Scheduler`, `SchedulerAdmission` |  |
| `TaskExtensions` | [basis/unit/task/mod.rs:59](../../server/src/basis/unit/task/mod.rs#L59) | `Scheduler` |  |
| `TaskMemory` | [basis/unit/task/identity.rs:78](../../server/src/basis/unit/task/identity.rs#L78) |  |  |
| `Registry` | [basis/unit/task/registry.rs:18](../../server/src/basis/unit/task/registry.rs#L18), [basis/unit/task/registry.rs:21](../../server/src/basis/unit/task/registry.rs#L21) | `TeamHeld` | `BlockPool`, `BlockPump`, `TeamState` |
| `TeamState` | [basis/unit/team/lifecycle/mod.rs:24](../../server/src/basis/unit/team/lifecycle/mod.rs#L24) | `BlockEpoch`, `Scheduler`, `TerminationQueue`, `Registry` |  |
| `TeamTasks` | [basis/unit/team/lifecycle/mod.rs:25](../../server/src/basis/unit/team/lifecycle/mod.rs#L25) | `Commit` | `TaskReceipts`, `TaskState`, `TeamHeld` |
| `TeamChildren` | [basis/unit/team/lifecycle/mod.rs:26](../../server/src/basis/unit/team/lifecycle/mod.rs#L26) | `Commit` | `TaskHeir` |
| `TeamTermination` | [basis/unit/team/lifecycle/mod.rs:30](../../server/src/basis/unit/team/lifecycle/mod.rs#L30) | `TerminationQueue` |  |
| `TeamHeld` | [basis/unit/team/membership.rs:14](../../server/src/basis/unit/team/membership.rs#L14) | `TeamTasks` | `Registry` |
| `TeamExit` | [basis/unit/team/lifecycle/exit.rs:22](../../server/src/basis/unit/team/lifecycle/exit.rs#L22) | `Commit` |  |
| `CloneQueue` | [basis/view/clone/mod.rs:33](../../server/src/basis/view/clone/mod.rs#L33) | `LedgerEntries` |  |
| `BootMmio` | [entry/boot/devices/entry_mmio.rs:23](../../server/src/entry/boot/devices/entry_mmio.rs#L23) |  | `BlockPool`, `BlockPump` |
| `BootRegions` | [entry/boot/devices/external.rs:13](../../server/src/entry/boot/devices/external.rs#L13) |  | `Commit` |
| `BootNests` | [entry/boot/devices/external.rs:17](../../server/src/entry/boot/devices/external.rs#L17) |  | `Nest` |
| `BlockPool` | [metal/allocator/block/inner.rs:17](../../server/src/metal/allocator/block/inner.rs#L17) | `PageMembership`, `SourceAuthorities`, `FaultPolicy`, `SpanExports`, `NestAuthority`, `SourceIndex`, `LedgerAdmission`, `LedgerHeirs`, `BlockUses`, `HeldSources`, `SourceFlights`, `SourceBindings`, `SourceOrigins`, `ProvenanceUses`, `Deadline`, `InterruptSources`, `SignRouting`, `WaitHolders`, `WaitSites`, `ControlTable`, `TaskReceipts`, `TaskHeir`, `Registry`, `BootMmio` | `Frame`, `Tally` |
| `BlockPump` | [metal/allocator/block/inner.rs:19](../../server/src/metal/allocator/block/inner.rs#L19) | `PageMembership`, `SourceAuthorities`, `FaultPolicy`, `SpanExports`, `NestAuthority`, `SourceIndex`, `LedgerAdmission`, `LedgerHeirs`, `BlockUses`, `HeldSources`, `SourceFlights`, `SourceBindings`, `SourceOrigins`, `ProvenanceUses`, `Deadline`, `InterruptSources`, `SignRouting`, `WaitHolders`, `WaitSites`, `ControlTable`, `TaskReceipts`, `TaskHeir`, `Registry`, `BootMmio` |  |
| `Tally` | [metal/allocator/block/tally.rs:10](../../server/src/metal/allocator/block/tally.rs#L10) | `BlockPool` |  |
| `Bump` | [metal/allocator/bump/mod.rs:15](../../server/src/metal/allocator/bump/mod.rs#L15) |  |  |
| `Frame` | [metal/allocator/frame/mod.rs:17](../../server/src/metal/allocator/frame/mod.rs#L17) | `TaskJournal`, `BlockPool`, `Asid` |  |
| `Spare` | [metal/allocator/spare/mod.rs:27](../../server/src/metal/allocator/spare/mod.rs#L27) |  |  |
| `Controllers` | [metal/riscv/iommu/controllers/mod.rs:10](../../server/src/metal/riscv/iommu/controllers/mod.rs#L10) | `Nest` | `PageMembership` |
| `Asid` | [metal/riscv/mmu/identity/asid.rs:13](../../server/src/metal/riscv/mmu/identity/asid.rs#L13) | `LedgerSerial` | `Frame` |
