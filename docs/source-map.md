# Kernel source navigation

This is a **lookup index**, not a second architectural model. Read
[the design](design/main.typ) for the problem and the reasons for each boundary.
Use [the Kernel API](../api/README.md) for current public contracts.

| Question | Public entry | Main enforcement / effect location |
| --- | --- | --- |
| Who executes, controls, waits, or returns a result? | [Unit calls](../api/src/abi/call/unit.rs), [Room calls](../api/src/abi/call/room.rs) | [Unit](../server/src/basis/unit/), [Room](../server/src/basis/room/) |
| What physical memory is held, associated, or published? | [Cell model](../api/src/model/cell/), [Cell calls](../api/src/abi/call/cell.rs) | [Cell](../server/src/basis/cell/), [Flat](../server/src/basis/flat/), [Nest](../server/src/basis/nest/) |
| What grants access across an isolation boundary? | [View model](../api/src/model/view/), [Pend model](../api/src/model/pend/) | [View](../server/src/basis/view/), [Pend](../server/src/basis/pend/) |
| What retains or routes a control event? | [Sign model](../api/src/model/sign/), [Sign calls](../api/src/abi/call/sign.rs) | [Sign](../server/src/basis/sign/) and [Room interrupt coordination](../server/src/basis/room/interrupt/) |
| How does a call reach its owner? | [ABI calls](../api/src/abi/call/) | [Entry](../server/src/entry/) |
| How is external access admitted and delegated? | [Nest model](../api/src/model/nest/), [Pend model](../api/src/model/pend/) | [Boot admission](../server/src/entry/boot/devices/external.rs), [Nest](../server/src/basis/nest/) and [RISC-V IOMMU](../server/src/metal/riscv/iommu/) |
| What proves that hardware effects are complete? | Selected [public contracts](../api/) and [trusted boot model](../api/src/boot/) | [Metal](../server/src/metal/), including the selected architecture backend |
| Where does a consumer observe global memory without gaining control? | [Versioned memory snapshot](../api/src/model/memory.rs), [Boot Ledger](../api/src/boot/ledger/) | [Kernel observer Task](../server/src/entry/observe/), [Allocator](../server/src/metal/allocator/frame/) and [Page retirement statistics](../server/src/basis/cell/page/reclaim/statistics.rs) |
| Where are these behaviors verified? | [API tests](../api/tests/) | [Server tests](../server/tests/), product scenarios in [Vanllia](https://github.com/shuyra-arche/vanllia) |

These are **navigational paths as of the current source layout**, not a
promise that each path corresponds to one architectural concept.

Trusted boot authenticates hardware requesters, admits External Nests, and
supplies their root Pends. Nest holds the address space, Pend bounds the
authority that can be delegated, and the IOMMU backend binds the requester
and supplies retirement evidence. See the [ABI migration notes](../api/README.md#migration-from-abi-v5--ledger-v7-to-abi-v6--ledger-v8)
for the change from Port.

The distinction between `basis`, `entry`, and `metal` describes
implementation responsibilities:

- `basis` owns resource and execution state and enforces local invariants.
- `entry` connects boot, trap, call dispatch, and reporting to their owners.
- `metal` provides architecture and platform effects and trusted observations.

These are **not** the chapter boundaries of the design narrative. Do not
duplicate each module's implementation details here.
