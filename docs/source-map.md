# Kernel source map

This is a **navigation index**, not another architecture or API
specification. Use the [design](design/main.typ) for reasons and the
[API](../api/README.md) for current version selection and callable contracts.

| Question | Public entry | Enforcing owner |
| --- | --- | --- |
| Who executes, stops, schedules and waits? | [Unit](../api/src/abi/call/unit.rs), [Room](../api/src/abi/call/room.rs) | [Unit](../server/src/basis/unit/), [Room](../server/src/basis/room/) |
| What holds and transfers Page storage? | [Memory contract](../api/memory.md), [Native Flat requests](../api/src/abi/memory/request.rs) | [Flat](../server/src/basis/flat/), [Page](../server/src/basis/cell/page/) |
| What reserves addresses and publishes access? | [native memory request](../api/src/abi/memory/request.rs) | [Nest](../server/src/basis/nest/) |
| Who delegates or revokes resource access? | [Pend](../api/src/abi/call/pend.rs), [scopes](../api/src/model/pend/scope.rs) | [Pend](../server/src/basis/pend/) |
| How are data and control delivered? | [Clone](../api/src/abi/call/view/clone.rs), [Share](../api/src/abi/call/view/share.rs), [Sign](../api/src/abi/call/sign.rs) | [View](../server/src/basis/view/), [Sign](../server/src/basis/sign/) |
| Who owns a Flat export shared by Flat Pend and Share? | [Share](../api/src/abi/call/view/share.rs) | [Flat source](../server/src/basis/flat/source/); [Pend source authority](../server/src/basis/pend/source/) owns live permission and [Nest](../server/src/basis/nest/) owns publication/retirement |
| Where is a call decoded? | [ABI](../api/src/abi/call/), [native Frame](../api/src/abi/memory/call.rs) | [Entry](../server/src/entry/) |
| Who accepts, replays and records memory requests? | [Memory requests](../api/src/abi/memory/request.rs) | [Task request owner](../server/src/basis/unit/task/request/); Entry copies/routes descriptors and resource owners enforce transitions |
| What supplies trusted external hardware facts? | [Boot](../api/src/boot/), [memory contract](../api/memory.md) | [Boot](../server/src/entry/boot/), [Metal](../server/src/metal/) |
| What retains retired resource responsibility? | [Memory](../api/memory.md#coherence-and-retirement) | [Nest translation](../server/src/basis/nest/translation/), [IOMMU](../server/src/metal/riscv/mmu/external/) |
| Who owns mandatory Task exit closure? | Internal Unit lifecycle | [Exit coordinator](../server/src/basis/unit/lifecycle/reap.rs); Unit and Pend retain their own termination/authorization state |
| What permits final shutdown? | Internal shutdown preparation and Hart barrier | [Shutdown owner](../server/src/basis/room/shutdown/), [entry finalization](../server/src/entry/boot/finalization.rs); optional observation grants no completion evidence |
| Where does deferred Unit cleanup execute? | Kernel-internal Task entry | [Lifecycle Task](../server/src/entry/work/lifecycle/); [Unit owner](../server/src/basis/unit/lifecycle/) retains queues, retirement stages and fallback |
| Who advances Page-completion discovery? | Kernel-internal Task entry | [Retirement Task](../server/src/entry/work/retirement/); [Nest coordinator](../server/src/basis/nest/retirement.rs), [source pass](../server/src/basis/nest/index/source.rs) and [Cell registry](../server/src/basis/cell/change.rs) retain progress |
| Who supplies global memory observations? | [Snapshot](../api/src/model/memory/mod.rs) | [Observer](../server/src/entry/work/observe/), [Allocator](../server/src/metal/allocator/) |
| How is behavior checked? | [API tests](../api/tests/) | [Kernel tests](../server/tests/), [Vanllia integration](https://github.com/shuyra-arche/vanllia) |

`basis` owns resource/execution state; `entry` connects boot,
trap and call dispatch; `metal` owns architecture effects and
trusted observations. Source ownership, authorization, publication
and safe reuse are distinct even when one call coordinates them.

Translation contracts are navigable from
[`generic/mmu`](../server/src/metal/generic/mmu/mod.rs):
[`internal`](../server/src/metal/generic/mmu/internal/mod.rs) covers CPU accesses
(including user mode), while
[`external`](../server/src/metal/generic/mmu/external/mod.rs) covers external
requesters. The complete RISC-V implementations live under the corresponding
[`riscv/mmu`](../server/src/metal/riscv/mmu/mod.rs) children. CPU shootdown
coverage and external controller/platform settlement remain distinct contracts.
[`generic/memory`](../server/src/metal/generic/memory/mod.rs) retains trusted
hardware visibility/topology proofs; Page membership and authorization remain
with basis. Neither is an external-only translation concern.

Synchronization has three owners:
[`generic/sync`](../server/src/metal/generic/sync/mod.rs) owns FIFO admission,
ordinary/reentrant storage, RAII guards, OnceLock and bounded debug checking;
[`riscv/sync`](../server/src/metal/riscv/sync/mod.rs) supplies the statically
selected IRQ/owner/wait environment;
[`basis/sync`](../server/src/basis/sync/mod.rs) owns resource categories, typed
ancestor rules, debug policy and the unique COMMIT storage. Transaction semantics
remain in [`basis/commit`](../server/src/basis/commit.rs).
