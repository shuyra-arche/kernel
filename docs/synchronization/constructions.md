# Construction-site snapshot

Source: `852b396d20a1ae2954503a4dee9d970658c43558`. This is textual discovery,
not a count of live instances or completed acquisition-chain evidence.
See the [audit status and remaining family work](README.md).

| # | Construction site | Legacy Level | Constructor source |
| --- | --- | --- | --- |
| 1 | [basis/cell/change.rs:9](../../server/src/basis/cell/change.rs#L9) | `L3` | `static LIVE: SpinLock<Vec<Weak<Change>>> = SpinLock::new_level(Level::L3, Vec::new());` |
| 2 | [basis/cell/page/mod.rs:95](../../server/src/basis/cell/page/mod.rs#L95) | `None` | `membership: crate::metal::sync::SpinLock::new(reclaim::group::Membership::new()),` |
| 3 | [basis/cell/page/reclaim/change.rs:36](../../server/src/basis/cell/page/reclaim/change.rs#L36) | `None` | `blocks: crate::metal::sync::SpinLock::new(Blocks {` |
| 4 | [basis/cell/page/reclaim/change.rs:143](../../server/src/basis/cell/page/reclaim/change.rs#L143) | `None` | `blocks: crate::metal::sync::SpinLock::new(Blocks {` |
| 5 | [basis/cell/page/reclaim/statistics.rs:11](../../server/src/basis/cell/page/reclaim/statistics.rs#L11) | `None` | `static TOTAL: SpinLock<Snapshot> = SpinLock::new(Snapshot {` |
| 6 | [basis/cell/page/shape.rs:68](../../server/src/basis/cell/page/shape.rs#L68) | `None` | `membership: crate::metal::sync::SpinLock::new(reclaim::group::Membership::new()),` |
| 7 | [basis/commit.rs:8](../../server/src/basis/commit.rs#L8) | `Unit` | `crate::metal::sync::RelLock::new_level(crate::metal::sync::Level::Unit, ());` |
| 8 | [basis/flat/mod.rs:122](../../server/src/basis/flat/mod.rs#L122) | `Flat` | `state: SpinLock::new_level(Level::Flat, FlatState::new(source, transferable)),` |
| 9 | [basis/flat/sharing/mod.rs:80](../../server/src/basis/flat/sharing/mod.rs#L80) | `None` | `authorization: SpinLock::new(None),` |
| 10 | [basis/flat/sharing/mod.rs:81](../../server/src/basis/flat/sharing/mod.rs#L81) | `None` | `maps: SpinLock::new(maps),` |
| 11 | [basis/flat/sharing/mod.rs:82](../../server/src/basis/flat/sharing/mod.rs#L82) | `L3` | `authorities: SpinLock::new_level(Level::L3, authorities),` |
| 12 | [basis/nest/activity.rs:51](../../server/src/basis/nest/activity.rs#L51) | `None` | `static REGISTRY: SpinLock<Registry> = SpinLock::new(Registry {` |
| 13 | [basis/nest/activity.rs:258](../../server/src/basis/nest/activity.rs#L258) | `None` | `delivery: SpinLock::new(Delivery {` |
| 14 | [basis/nest/address/span/mod.rs:137](../../server/src/basis/nest/address/span/mod.rs#L137) | `None` | `faults: SpinLock::new(super::super::fault::State::new()),` |
| 15 | [basis/nest/address/span/mod.rs:141](../../server/src/basis/nest/address/span/mod.rs#L141) | `None` | `exports: SpinLock::new(crate::basis::pend::Tracking::new()),` |
| 16 | [basis/nest/address/span/mod.rs:147](../../server/src/basis/nest/address/span/mod.rs#L147) | `Reservation` | `state: SpinLock::new_level(Level::Reservation, SpanState::new(active, source)),` |
| 17 | [basis/nest/authority/binding.rs:35](../../server/src/basis/nest/authority/binding.rs#L35) | `None` | `static ORGANIZATIONS: SpinLock<Vec<Organization>> = SpinLock::new(Vec::new());` |
| 18 | [basis/nest/authority/mod.rs:111](../../server/src/basis/nest/authority/mod.rs#L111) | `None` | `ranges: SpinLock::new(Ranges {` |
| 19 | [basis/nest/fault/queue.rs:81](../../server/src/basis/nest/fault/queue.rs#L81) | `None` | `progress: SpinLock::new(Progress::Pending { publication: None }),` |
| 20 | [basis/nest/index/source.rs:44](../../server/src/basis/nest/index/source.rs#L44) | `L3` | `head: SpinLock::new_level(Level::L3, None),` |
| 21 | [basis/nest/memo/activity/mod.rs:108](../../server/src/basis/nest/memo/activity/mod.rs#L108) | `None` | `history: SpinLock::new(History::new()),` |
| 22 | [basis/nest/mod.rs:240](../../server/src/basis/nest/mod.rs#L240) | `Address` | `layout: SpinLock::new_level(Level::Address, layout),` |
| 23 | [basis/nest/mod.rs:241](../../server/src/basis/nest/mod.rs#L241) | `Address` | `pending_cleanup: SpinLock::new_level(Level::Address, None),` |
| 24 | [basis/nest/mod.rs:242](../../server/src/basis/nest/mod.rs#L242) | `Nest` | `state: ManuallyDrop::new(SpinLock::new_level(Level::Nest, NestState::new(root))),` |
| 25 | [basis/nest/translation/external/mod.rs:53](../../server/src/basis/nest/translation/external/mod.rs#L53) | `Address` | `layout: SpinLock::new_level(Level::Address, Layout::new()),` |
| 26 | [basis/nest/translation/external/mod.rs:54](../../server/src/basis/nest/translation/external/mod.rs#L54) | `None` | `pending_cleanup: SpinLock::new(None),` |
| 27 | [basis/nest/translation/external/mod.rs:55](../../server/src/basis/nest/translation/external/mod.rs#L55) | `Nest` | `state: ManuallyDrop::new(SpinLock::new_level(Level::Nest, NestState::external())),` |
| 28 | [basis/pend/authority/mod.rs:65](../../server/src/basis/pend/authority/mod.rs#L65) | `None` | `state: SpinLock::new(State::authorized(access, scope, operations, distributable)),` |
| 29 | [basis/pend/authority/mod.rs:66](../../server/src/basis/pend/authority/mod.rs#L66) | `None` | `parent: SpinLock::new(None),` |
| 30 | [basis/pend/ledger/admission.rs:36](../../server/src/basis/pend/ledger/admission.rs#L36) | `None` | `usage: SpinLock::new(Usage {` |
| 31 | [basis/pend/ledger/entry.rs:231](../../server/src/basis/pend/ledger/entry.rs#L231) | `None` | `charge: crate::metal::sync::SpinLock::new(None),` |
| 32 | [basis/pend/ledger/table.rs:32](../../server/src/basis/pend/ledger/table.rs#L32) | `None` | `serial: SpinLock::new(()),` |
| 33 | [basis/pend/ledger/table.rs:35](../../server/src/basis/pend/ledger/table.rs#L35) | `None` | `entries: SpinLock::new(Registry::default()),` |
| 34 | [basis/pend/ledger/table.rs:36](../../server/src/basis/pend/ledger/table.rs#L36) | `None` | `heirs: SpinLock::new(Heirs {` |
| 35 | [basis/pend/source/admission.rs:429](../../server/src/basis/pend/source/admission.rs#L429) | `None` | `pages: SpinLock::new(Vec::new()),` |
| 36 | [basis/pend/source/admission.rs:430](../../server/src/basis/pend/source/admission.rs#L430) | `None` | `native: SpinLock::new(None),` |
| 37 | [basis/pend/source/admission.rs:431](../../server/src/basis/pend/source/admission.rs#L431) | `None` | `uses: SpinLock::new(Vec::new()),` |
| 38 | [basis/pend/source/admission.rs:432](../../server/src/basis/pend/source/admission.rs#L432) | `None` | `stages: SpinLock::new(Vec::new()),` |
| 39 | [basis/pend/source/hold.rs:150](../../server/src/basis/pend/source/hold.rs#L150) | `None` | `holds: SpinLock::new(Vec::new()),` |
| 40 | [basis/pend/source/hold.rs:151](../../server/src/basis/pend/source/hold.rs#L151) | `None` | `sources: SpinLock::new(Tracking::new()),` |
| 41 | [basis/pend/source/mod.rs:115](../../server/src/basis/pend/source/mod.rs#L115) | `None` | `inflight: SpinLock::new(Vec::new()),` |
| 42 | [basis/pend/source/mod.rs:116](../../server/src/basis/pend/source/mod.rs#L116) | `None` | `revoked: SpinLock::new(false),` |
| 43 | [basis/pend/source/mod.rs:117](../../server/src/basis/pend/source/mod.rs#L117) | `None` | `bindings: SpinLock::new(Vec::new()),` |
| 44 | [basis/pend/source/mod.rs:118](../../server/src/basis/pend/source/mod.rs#L118) | `None` | `blocks: SpinLock::new(Vec::new()),` |
| 45 | [basis/pend/source/mod.rs:119](../../server/src/basis/pend/source/mod.rs#L119) | `None` | `origins: SpinLock::new(Vec::new()),` |
| 46 | [basis/pend/source/mod.rs:120](../../server/src/basis/pend/source/mod.rs#L120) | `None` | `phase: SpinLock::new(Phase::Private(range)),` |
| 47 | [basis/pend/source/provenance.rs:107](../../server/src/basis/pend/source/provenance.rs#L107) | `None` | `grant: SpinLock::new(Carried::new(` |
| 48 | [basis/pend/source/provenance.rs:118](../../server/src/basis/pend/source/provenance.rs#L118) | `None` | `uses: SpinLock::new(Vec::new()),` |
| 49 | [basis/room/deadline/mod.rs:43](../../server/src/basis/room/deadline/mod.rs#L43) | `L3` | `TIMERS.get_or_init(\|\| SpinLock::new_level(Level::L3, Queue::new()))` |
| 50 | [basis/room/interrupt/mod.rs:111](../../server/src/basis/room/interrupt/mod.rs#L111) | `None` | `controller: SpinLock::new(controller),` |
| 51 | [basis/room/interrupt/mod.rs:112](../../server/src/basis/room/interrupt/mod.rs#L112) | `None` | `progress: SpinLock::new(Progress::new()),` |
| 52 | [basis/room/interrupt/mod.rs:127](../../server/src/basis/room/interrupt/mod.rs#L127) | `None` | `sources: SpinLock::new(Vec::new()),` |
| 53 | [basis/room/interrupt/source/mod.rs:48](../../server/src/basis/room/interrupt/source/mod.rs#L48) | `L3` | `state: SpinLock::new_level(Level::L3, State::new()),` |
| 54 | [basis/room/scheduler/slot/mod.rs:57](../../server/src/basis/room/scheduler/slot/mod.rs#L57) | `Scheduler` | `inner: SpinLock::new_level(` |
| 55 | [basis/room/scheduler/slot/mod.rs:66](../../server/src/basis/room/scheduler/slot/mod.rs#L66) | `None` | `admission: SpinLock::new(super::contract::Admission::new()),` |
| 56 | [basis/room/shutdown/mod.rs:22](../../server/src/basis/room/shutdown/mod.rs#L22) | `None` | `static PROGRESS: SpinLock<Progress> = SpinLock::new(Progress::Idle);` |
| 57 | [basis/sign/endpoint/delivery.rs:13](../../server/src/basis/sign/endpoint/delivery.rs#L13) | `None` | `static PENDING: SpinLock<Queue> = SpinLock::new(Queue {` |
| 58 | [basis/sign/endpoint/mod.rs:92](../../server/src/basis/sign/endpoint/mod.rs#L92) | `L3` | `state: SpinLock::new_level(Level::L3, State::new()),` |
| 59 | [basis/sign/endpoint/mod.rs:100](../../server/src/basis/sign/endpoint/mod.rs#L100) | `L3` | `routing: SpinLock::new_level(Level::L3, Routing::Sources(Vec::new())),` |
| 60 | [basis/sign/endpoint/mod.rs:103](../../server/src/basis/sign/endpoint/mod.rs#L103) | `None` | `delivery: SpinLock::new(delivery::Delivery::Idle),` |
| 61 | [basis/sign/wait/holder.rs:34](../../server/src/basis/sign/wait/holder.rs#L34) | `L3` | `HOLDERS.get_or_init(\|\| SpinLock::new_level(Level::L3, HashMap::new()))` |
| 62 | [basis/sign/wait/site.rs:131](../../server/src/basis/sign/wait/site.rs#L131) | `L3` | `v.push(SpinLock::new_level(Level::L3, HashMap::new()));` |
| 63 | [basis/unit/control/table.rs:38](../../server/src/basis/unit/control/table.rs#L38) | `None` | `Self(SpinLock::new(Registry::default()))` |
| 64 | [basis/unit/lifecycle/diagnostics/mod.rs:12](../../server/src/basis/unit/lifecycle/diagnostics/mod.rs#L12) | `None` | `static LEDGER: SpinLock<Ring> = SpinLock::new(Ring::new());` |
| 65 | [basis/unit/lifecycle/doom.rs:83](../../server/src/basis/unit/lifecycle/doom.rs#L83) | `None` | `static PENDING: crate::metal::sync::SpinLock<Queue> = crate::metal::sync::SpinLock::new(Queue {` |
| 66 | [basis/unit/lifecycle/reap.rs:25](../../server/src/basis/unit/lifecycle/reap.rs#L25) | `None` | `static PENDING: SpinLock<Chain> = SpinLock::new(Chain::new());` |
| 67 | [basis/unit/lifecycle/reap.rs:26](../../server/src/basis/unit/lifecycle/reap.rs#L26) | `L3` | `pub(super) static HUSKS: SpinLock<Husks> = SpinLock::new_level(Level::L3, Husks::new());` |
| 68 | [basis/unit/task/creation/mod.rs:214](../../server/src/basis/unit/task/creation/mod.rs#L214) | `UnitState` | `let state = Arc::try_new(SpinLock::new_level(Level::UnitState, TaskState::held()))` |
| 69 | [basis/unit/task/creation/prepared.rs:73](../../server/src/basis/unit/task/creation/prepared.rs#L73) | `None` | `receipts: SpinLock::new(receipts),` |
| 70 | [basis/unit/task/creation/prepared.rs:74](../../server/src/basis/unit/task/creation/prepared.rs#L74) | `None` | `result: SpinLock::new(None),` |
| 71 | [basis/unit/task/creation/prepared.rs:119](../../server/src/basis/unit/task/creation/prepared.rs#L119) | `None` | `memory_results: SpinLock::new(None),` |
| 72 | [basis/unit/task/creation/prepared.rs:121](../../server/src/basis/unit/task/creation/prepared.rs#L121) | `None` | `heir: SpinLock::new(Vec::new()),` |
| 73 | [basis/unit/task/creation/prepared.rs:122](../../server/src/basis/unit/task/creation/prepared.rs#L122) | `None` | `retirement: SpinLock::new(super::super::retirement::Retirement::new()),` |
| 74 | [basis/unit/task/creation/prepared.rs:123](../../server/src/basis/unit/task/creation/prepared.rs#L123) | `None` | `schedule: SpinLock::new(super::super::schedule::Schedule::new()),` |
| 75 | [basis/unit/task/creation/prepared.rs:124](../../server/src/basis/unit/task/creation/prepared.rs#L124) | `None` | `extensions: SpinLock::new(` |
| 76 | [basis/unit/task/identity.rs:94](../../server/src/basis/unit/task/identity.rs#L94) | `None` | `memory: SpinLock::new(memory),` |
| 77 | [basis/unit/task/registry.rs:22](../../server/src/basis/unit/task/registry.rs#L22) | `Registry` | `TABLE.get_or_init(\|\| SpinLock::new_level(Level::Registry, HashMap::new()))` |
| 78 | [basis/unit/team/builder.rs:147](../../server/src/basis/unit/team/builder.rs#L147) | `UnitState` | `let state = SpinLock::new_level(` |
| 79 | [basis/unit/team/builder.rs:155](../../server/src/basis/unit/team/builder.rs#L155) | `TeamTasks` | `let tasks = SpinLock::new_level(Level::TeamTasks, Vec::new());` |
| 80 | [basis/unit/team/builder.rs:162](../../server/src/basis/unit/team/builder.rs#L162) | `None` | `children: SpinLock::new(Vec::new()),` |
| 81 | [basis/unit/team/builder.rs:166](../../server/src/basis/unit/team/builder.rs#L166) | `None` | `termination: SpinLock::new(crate::basis::unit::lifecycle::Termination::new()),` |
| 82 | [basis/unit/team/builder.rs:182](../../server/src/basis/unit/team/builder.rs#L182) | `L3` | `held: SpinLock::new_level(Level::L3, Vec::new()),` |
| 83 | [basis/unit/team/builder.rs:185](../../server/src/basis/unit/team/builder.rs#L185) | `None` | `operation: SpinLock::new(()),` |
| 84 | [basis/unit/team/lifecycle/exit.rs:29](../../server/src/basis/unit/team/lifecycle/exit.rs#L29) | `L3` | `state: SpinLock::new_level(` |
| 85 | [basis/view/clone/mod.rs:53](../../server/src/basis/view/clone/mod.rs#L53) | `L3` | `pending: SpinLock::new_level(Level::L3, Pending::Queue(Queue::default())),` |
| 86 | [entry/boot/devices/entry_mmio.rs:23](../../server/src/entry/boot/devices/entry_mmio.rs#L23) | `None` | `static STAGED: SpinLock<Layout> = SpinLock::new(Layout::Open(Vec::new()));` |
| 87 | [entry/boot/devices/external.rs:13](../../server/src/entry/boot/devices/external.rs#L13) | `None` | `pub(super) static REGIONS: SpinLock<Vec<Region>> = SpinLock::new(Vec::new());` |
| 88 | [entry/boot/devices/external.rs:16](../../server/src/entry/boot/devices/external.rs#L16) | `None` | `pub(super) static NESTS: SpinLock<Vec<(Name, Arc<Nest>)>> = SpinLock::new(Vec::new());` |
| 89 | [metal/allocator/block/inner.rs:26](../../server/src/metal/allocator/block/inner.rs#L26) | `None` | `pool: crate::metal::sync::SpinLock::new(pool),` |
| 90 | [metal/allocator/block/inner.rs:27](../../server/src/metal/allocator/block/inner.rs#L27) | `None` | `pump: core::array::from_fn(\|_\| crate::metal::sync::SpinLock::new(Pump::new())),` |
| 91 | [metal/allocator/block/tally.rs:22](../../server/src/metal/allocator/block/tally.rs#L22) | `Tally` | `guard: crate::metal::sync::SpinLock::new_level(crate::metal::sync::Level::Tally, ()),` |
| 92 | [metal/allocator/bump/mod.rs:21](../../server/src/metal/allocator/bump/mod.rs#L21) | `None` | `inner: crate::metal::sync::SpinLock::new(None),` |
| 93 | [metal/allocator/frame/mod.rs:25](../../server/src/metal/allocator/frame/mod.rs#L25) | `Frame` | `inner: crate::metal::sync::SpinLock::new_level(crate::metal::sync::Level::Frame, inner),` |
| 94 | [metal/allocator/spare/mod.rs:42](../../server/src/metal/allocator/spare/mod.rs#L42) | `Spare` | `inner: crate::metal::sync::SpinLock::new_level(` |
| 95 | [metal/riscv/iommu/controllers/mod.rs:10](../../server/src/metal/riscv/iommu/controllers/mod.rs#L10) | `None` | `pub(crate) static CONTROLLERS: SpinLock<Option<Controllers<Mmio>>> = SpinLock::new(None);` |
| 96 | [metal/riscv/mmu/identity/asid.rs:14](../../server/src/metal/riscv/mmu/identity/asid.rs#L14) | `Asid` | `static ASID_ALLOCATOR: SpinLock<Recycle> = SpinLock::new_level(Level::Asid, Recycle::new(1, 65536));` |
