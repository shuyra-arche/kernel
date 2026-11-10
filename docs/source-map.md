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
| What proves that hardware effects are complete? | Selected [public contracts](../api/) and [trusted boot model](../api/src/boot/) | [Metal](../server/src/metal/), including the selected architecture backend |
| Where are these behaviors verified? | [API tests](../api/tests/) | [Server tests](../server/tests/), product scenarios in [Vanllia](https://github.com/shuyra-arche/vanllia) |

These are **navigational paths as of the current source layout**, not a
promise that each path corresponds to one architectural concept.

The current code still exposes a public
[Port model](../api/src/model/port/) and
[Port calls](../api/src/abi/call/port.rs). Their long-term public
necessity is disputed in [Issue #124](https://github.com/shuyra-arche/kernel/issues/124).
The Kernel must continue to authenticate real hardware requesters and retain
hardware settlement responsibilities regardless of that decision.

The distinction between `basis`, `entry`, and `metal` describes
implementation responsibilities:

- `basis` owns resource and execution state and enforces local invariants.
- `entry` connects boot, trap, call dispatch, and reporting to their owners.
- `metal` provides architecture and platform effects and trusted observations.

These are **not** the chapter boundaries of the design narrative. Do not
duplicate each module's implementation details here.
