# Kernel source map

This is a **navigation index**, not another architecture or API
specification. Use the [design](design/main.typ) for reasons and the
[API](../api/README.md) for current version selection and callable contracts.

| Question | Public entry | Enforcing owner |
| --- | --- | --- |
| Who executes, stops, schedules and waits? | [Unit](../api/src/abi/call/unit.rs), [Room](../api/src/abi/call/room.rs) | [Unit](../server/src/basis/unit/), [Room](../server/src/basis/room/) |
| What holds and transfers Page storage? | [Memory contract](../api/memory.md), [Cell calls](../api/src/abi/call/cell.rs) | [Flat](../server/src/basis/flat/), [Page](../server/src/basis/cell/page/) |
| What reserves addresses and publishes access? | [Nest](../api/src/abi/call/nest.rs), [native memory request](../api/src/abi/memory/request.rs) | [Nest](../server/src/basis/nest/) |
| Who delegates or revokes resource access? | [Pend](../api/src/abi/call/pend.rs), [scopes](../api/src/model/pend/scope.rs) | [Pend](../server/src/basis/pend/) |
| How are data and control delivered? | [Clone](../api/src/abi/call/view/clone.rs), [Share](../api/src/abi/call/view/share.rs), [Sign](../api/src/abi/call/sign.rs) | [View](../server/src/basis/view/), [Sign](../server/src/basis/sign/) |
| Where is a call decoded? | [ABI](../api/src/abi/call/), [native Frame](../api/src/abi/memory/call.rs) | [Entry](../server/src/entry/) |
| What supplies trusted external hardware facts? | [Boot](../api/src/boot/), [memory contract](../api/memory.md) | [Boot](../server/src/entry/boot/), [Metal](../server/src/metal/) |
| What retains retired resource responsibility? | [Memory](../api/memory.md#coherence-and-retirement) | [Nest translation](../server/src/basis/nest/translation/), [IOMMU](../server/src/metal/riscv/iommu/) |
| Who supplies global memory observations? | [Snapshot](../api/src/model/memory.rs) | [Observer](../server/src/entry/observe/), [Allocator](../server/src/metal/allocator/) |
| How is behavior checked? | [API tests](../api/tests/) | [Kernel tests](../server/tests/), [Vanllia integration](https://github.com/shuyra-arche/vanllia) |

`basis` owns resource/execution state; `entry` connects boot,
trap and call dispatch; `metal` owns architecture effects and
trusted observations. Source ownership, authorization, publication
and safe reuse are distinct even when one call coordinates them.
