# Kernel source navigation

This is a **lookup index**, not a second architectural model. Read
[the design](design/main.typ) for the problem and the reasons for each boundary.
Use [the Kernel API](../api/README.md) for current public contracts.

| Question | Public entry | Main enforcement / effect location |
| --- | --- | --- |
| Who executes, controls, waits, or returns a result? | [Unit calls](../api/src/abi/call/unit.rs), [Room calls](../api/src/abi/call/room.rs) | [Unit](../server/src/basis/unit/), [Room](../server/src/basis/room/) |
| What physical memory is held, associated, or published? | [Cell model](../api/src/model/cell/), [Cell calls](../api/src/abi/call/cell.rs) | [Cell](../server/src/basis/cell/), [Flat](../server/src/basis/flat/), [Nest](../server/src/basis/nest/) |
| Where do source-free mapping Reserve / Release commit? | [Nest calls](../api/src/abi/call/nest.rs), [ReserveRequest](../api/src/model/nest/reserve.rs) | [Nest entry](../server/src/entry/call/nest.rs), [Prepared reservation](../server/src/basis/nest/address/reservation.rs), [Span](../server/src/basis/nest/address/span/) and [Nest namespace registry](../server/src/basis/nest/authority/) |
| What grants access across an isolation boundary? | [View model](../api/src/model/view/), [Pend model](../api/src/model/pend/) | [View](../server/src/basis/view/), [Pend](../server/src/basis/pend/) |
| Where are Nest Whole / Mapping scopes and their real ancestry checked? | [Typed scopes](../api/src/model/pend/scope.rs), [Limits and rights](../api/src/model/pend/permission.rs) | [Scoped access](../server/src/basis/pend/access.rs), [Grant preparation](../server/src/basis/pend/ledger/entry.rs), [Accord / Narrow](../server/src/basis/pend/branch.rs) and [Branch cleanup](../server/src/basis/pend/ledger/cleanup.rs) |
| What retains a position's original source, generation and publication? | [Cell positions and associations](../api/src/model/cell/object.rs) | [Nest-owned Memo state](../server/src/basis/nest/memo/), [Original source](../server/src/basis/nest/memo/source.rs) and [Publishing route](../server/src/basis/nest/memo/publisher.rs) |
| What retains or routes a control event? | [Sign model](../api/src/model/sign/), [Sign calls](../api/src/abi/call/sign.rs) | [Sign](../server/src/basis/sign/) and [Room interrupt coordination](../server/src/basis/room/interrupt/) |
| How does a call reach its owner? | [ABI calls](../api/src/abi/call/) | [Entry](../server/src/entry/) |
| How is external access admitted and delegated? | [Nest model](../api/src/model/nest/), [Pend model](../api/src/model/pend/) | [Boot admission](../server/src/entry/boot/devices/external.rs), [Trusted Whole binding](../server/src/basis/nest/authority/binding.rs) and [RISC-V IOMMU](../server/src/metal/riscv/iommu/) |
| Where do CPU Whole grants come from? | [CreatedUnit](../api/src/model/unit/creation.rs), [Ledger names](../api/src/boot/ledger/name.rs) | [Team creation / adoption](../server/src/entry/call/unit/mod.rs) and [Boot Space grant](../server/src/entry/boot/resources/authority.rs) |
| What proves that hardware effects are complete? | Selected [public contracts](../api/) and [trusted boot model](../api/src/boot/) | [Metal](../server/src/metal/), including the selected architecture backend |
| Where does a consumer observe global memory without gaining control? | [Versioned memory snapshot](../api/src/model/memory.rs), [Boot Ledger](../api/src/boot/ledger/) | [Kernel observer Task](../server/src/entry/observe/), [Allocator](../server/src/metal/allocator/frame/) and [Page retirement statistics](../server/src/basis/cell/page/reclaim/statistics.rs) |
| Where are these behaviors verified? | [API tests](../api/tests/) | [Server tests](../server/tests/), product scenarios in [Vanllia](https://github.com/shuyra-arche/vanllia) |

These are **navigational paths as of the current source layout**, not a
promise that each path corresponds to one architectural concept.

Trusted boot authenticates hardware requesters, admits External Nests, and
supplies their Whole root Pends. Nest owns the mapping namespace and internal
per-position Memo state; Pend bounds the live Whole or Mapping authority that
can be delegated. The IOMMU backend binds the requester and supplies retirement
evidence. See the [ABI 10 migration](../api/README.md#migration-from-abi-v9--ledger-v9-to-abi-v10--ledger-v10)
for scoped Nest authority and the [historical Port migration](../api/README.md#historical-migration-from-abi-v5--ledger-v7-to-abi-v6--ledger-v8)
for the external binding boundary.

The distinction between `basis`, `entry`, and `metal` describes
implementation responsibilities:

- `basis` owns resource and execution state and enforces local invariants.
- `entry` connects boot, trap, call dispatch, and reporting to their owners.
- `metal` provides architecture and platform effects and trusted observations.

These are **not** the chapter boundaries of the design narrative. Do not
duplicate each module's implementation details here.
