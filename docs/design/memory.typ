#import "memory/diagram.typ": amber, drawing, green, label, node, wire
#import "style.typ": design-table

#set par(justify: false)

= Memory from allocation to reuse <memory>

A physical allocation can outlive its original holder, an address can
remain reserved without backing, and a device can still hold work after
the Task that submitted it exits. To decide when memory is safe to reuse,
the Kernel needs three independent facts: the physical resource and its
return obligation, its logical holding, and the translation that grants
access through an address.

Cell, Flat and Nest give each of these facts its own authority. The
#link("https://github.com/shuyra-arche/kernel/blob/main/api/memory.md")[current memory contract]
documents ABI 14's Page, Slot, Memo and Publication. The target changes
their organization and behavior, including the admission rules for
mapping and Transfer. The API, Client and consumers migrate together
when those contracts are implemented.

#include "memory/model.typ"

== Composing Cell, Flat and Nest <memory-composition>

=== Shared memory

A Flat holds X once. The source and destination permits can authorize
two Nests to map the same Cell interval at different addresses and
permissions. Each mapping owns its own CellAccess and translation
obligation. Withdrawing one settles only that mapping. Compatible alias
attributes, memory ordering and data synchronization still apply to the
whole participating group. Ordinary RAM sharing needs no extra Share
object. Specialized Share views, such as provider register windows, and
Activity sampling sessions keep their separate contracts.

#drawing(70mm)[
  #label(0mm, 0mm, [One disposition, independently retained accesses])
  #node(0mm, 9mm, 48mm, 25mm, [Flat], [Holding owns X.], fill: green)
  #node(56mm, 9mm, 48mm, 25mm, [Nest A], [VA A reads X.])
  #node(112mm, 9mm, 48mm, 25mm, [Nest B], [VA B reads or writes X.])
  #wire(((24mm, 34mm), (24mm, 42mm), (68mm, 42mm), (68mm, 48mm)))
  #wire(((80mm, 34mm), (80mm, 48mm)))
  #wire(((136mm, 34mm), (136mm, 42mm), (92mm, 42mm), (92mm, 48mm)))
  #node(56mm, 48mm, 48mm, 21mm, [Cell X], [One physical instance.], fill: green)
]

=== Copy and Task

Copy reads an admitted source slice into an independent Internal Cell
Y. The message owns the payload holding until delivery according to
the Copy protocol. A receiver gets a read-only Flat permit and can map
the payload through an authorized Nest. Source X and payload Y have
different physical identities and return responsibilities. Copy owns
queue and delivery state, including rollback of an already claimed
queue entry when delivery preparation fails and the queue stays open.

A loader maps code and stacks into an Internal Nest, waits for readiness
and then creates and starts the Task as permitted by the execution
contract. Tasks can share an Internal Nest and use separate stacks.
Unit owns Task creation and lifecycle results. Room owns scheduling
and execution placement. A Task binding must serialize with Nest
closure and grants no Nest management authority. Task exit can start
cleanup under the relevant lifetime contract. Previously admitted work
still has its original owner.

=== DMA and driver responsibility

A driver arranges an authorized requester binding and maps a suitable
buffer in an External Nest. Once binding and mapping are ready, it puts
the I/O address in a device descriptor. The device initiates the
access. A CPU mapping can coexist when the platform qualifies the
attributes and synchronization for all participants. The driver owns
device submission, the available device-side stop or drain procedure,
and the device's data-visibility protocol. The External backend owns
translation and binding retirement and verifies the required evidence
before ending the CellAccess. IRQ or Sign delivery can report
progress. A completed DMA request alone does not settle the mapping.

=== Swap and fault recovery

The user-space Swap Unit owns logical page identity, backing location,
replacement policy and aliases it is authorized to manage. For swap-out,
it closes ordinary new admissions and removes its managed old mappings,
including device aliases and in-flight writes. It keeps a separately
accounted access for the backing write, waits for successful storage I/O
and settles that access before returning X. A failed write leaves the
data and return responsibility available for recovery. If an alias
cannot be stopped or drained, the Unit cannot yet evict the page.
Transferring X into a private Flat moves disposition while existing
accesses remain live.

For swap-in, an authorized fault handler joins concurrent requests for
the same logical page where applicable, fills a new Cell Y and rechecks
the live reservation, route generation and rights before installing it.
Managed aliases of the same page can again bind the same Cell while
keeping their own permissions. The logical page, backing location and
valid route can persist while the resident Cell changes. The Task
resumes after installation is ready. No route follows from a Flat hole.
Recoverable faults in an External Nest require qualified hardware and
requester support.

== Concurrency, failures and recovery <memory-races>

Synchronization protects the facts that must agree at the operation's
commit. Flat coordinates a movement of holdings. Map coordinates the
live Flat and Nest facts with the relevant Pend admission gates. The
backends coordinate their own hardware effects. No cross-object lock
layout follows solely from these responsibilities.

#design-table(
  table.header([*Race or failure*], [*Result and owner*]),
  [Map and Transfer], [If Transfer commits first, a request using the old
    HeldAt fails final admission and releases any candidate access. If
    Map is admitted first, Nest keeps access to X while installation
    proceeds. Later placement of Y has no effect on that Mapping.],
  [Unmap and Release], [Unmap retains its address and CellAccess until
    hardware settlement. Nest Release cannot hand that address to a
    successor meanwhile. Flat Release may return disposition earlier,
    but Cell still waits for the old mapping and every other access.],
  [Admission and revocation], [Recheck all live gates at final admission.
    Close first rejects the use. Admission first records any contractual
    continuing dependency and makes the use discoverable for retirement.
    If both gates close, the original use retires once. Dropping a
    handle or transferring a holding does not revoke a branch.],
  [Stale evidence], [Nest and the backend match the context, exact
    mapping or binding, operation generation, hardware sequence and
    covered scope. A batch settles only its covered members. Reject
    stale evidence and make duplicates harmless. Fault and maintenance
    work likewise compares its own epochs before modifying live state.],
  [Caller exit], [Pre-effect candidates can be canceled. Admitted work
    with possible effects remains owned by its Mapping, binding,
    retirement, provider-return or other actual operation record. A
    Kernel worker may advance the record after the caller leaves.],
  [OOM or full queues], [Before admission reserve bounded space for
    effects, bookkeeping and recovery. Pre-commit failure preserves
    input ownership. A failure after possible effects leaves an exact
    recoverable record. Delivery failure does not undo a commit, and
    replay follows the original RequestId and its operation contract.],
  [Provider uncertainty], [The Cell return owner retains the original
    receipt while acceptance is uncertain. Retry only as allowed by the
    provider's protocol. If evidence remains unavailable, keep the
    obligation visible and storage unavailable for reuse.],
)

Persistent capacity is charged to the resource that owns the record.
A single operation must bound its temporary work and any reserved
recovery space. Waiting for a hart, controller, device or provider
occurs outside the address and tree commit scopes. A wakeup schedules
a check and the recoverable record remains the source of truth.

== DMA retirement after worker exit <memory-exit>

The worker has mapped X through an External Nest and has a queued
write. Its execution ends before the device access is settled. The
manager retains authority over the Nest and retires the mapping
under the applicable authorization and device contracts.

#drawing(93mm)[
  #node(0mm, 0mm, 76mm, 22mm, [Task exits], [
    The Task can publish its result.
  ])
  #node(85mm, 0mm, 75mm, 22mm, [A request is still in flight], [
    It can still try to reach X.
  ], fill: amber)
  #wire(((37mm, 22mm), (37mm, 27mm), (74mm, 27mm), (74mm, 32mm)))
  #wire(((122mm, 22mm), (122mm, 27mm), (86mm, 27mm), (86mm, 32mm)))
  #node(37mm, 32mm, 86mm, 26mm, [The mapping is Removing], [
    Nest retains the address and CellAccess while the backend checks
    invalidation, ordering and device drain.
  ], fill: amber)
  #wire(((80mm, 58mm), (80mm, 63mm), (37mm, 63mm), (37mm, 69mm)))
  #wire(((80mm, 63mm), (122mm, 63mm), (122mm, 69mm)))
  #node(0mm, 69mm, 76mm, 23mm, [Address handover], [
    After settlement and Release, a new reservation can use the address.
  ], fill: green)
  #node(85mm, 69mm, 75mm, 23mm, [Physical reuse], [
    X also needs returned disposition and settlement of every other use.
  ], fill: green)
]

Until the backend accepts exact retirement evidence, the address
cannot be released to a successor. Even after that mapping settles,
the Flat can still hold X or a CPU alias can still have access to it.
The #link(<memory-cell>)[Cell contract] governs final return, using the
evidence described under #link(<memory-mmu>)[MMU completion].

== Architecture and remaining work <memory-status>

*Established design.* Cell owns physical identity and the path to
return. Flat owns stable logical coordinates and unique holdings.
Nest owns address occupancy and mappings to exact admitted accesses.
Transfer preserves existing aliases. CPU and I/O backend evidence is
specific to the hardware effects. Table pages follow Cell reclamation.

*Implementation contracts.* Establish the exact CellAccess
representation and admission bookkeeping, including where platform
exclusivity is enforced. Implement bounded sparse-index preparation and
joint admission with live authorization. Define public operation,
replay and progress results, including partial retirement, fault refill,
and a recovery owner for each admitted effect. Resolve details of
Nest-to-Task and requester lifetime, authority and shutdown with the
relevant owners. Validate the affected migration paths before removing
ABI 14 mechanisms.

*Platform qualification.* Verify CPU activation, multi-hart
synchronization, label reuse and old table walks. Qualify supported
IOMMU controller and requester topologies, applicable device-cache
invalidation, upstream drain and coherency. Qualify external provider
lifetime, return and Split/Merge support separately. Host checks can
verify authorization, races, fault injection and record ownership.
Target execution checks the actual translation backends under its test
configuration. Physical device proofs require the relevant hardware
and platform contract.

Implementation and acceptance are tracked in
#link("https://github.com/shuyra-arche/kernel/issues/326")[the migration index]
and the related
#link("https://github.com/shuyra-arche/kernel/issues/330")[Cell],
#link("https://github.com/shuyra-arche/kernel/issues/331")[Flat] and
#link("https://github.com/shuyra-arche/kernel/issues/332")[Nest] issues.
Their boundaries with
#link("https://github.com/shuyra-arche/kernel/issues/327")[Sync],
#link("https://github.com/shuyra-arche/kernel/issues/328")[Execution],
#link("https://github.com/shuyra-arche/kernel/issues/329")[Pend],
#link("https://github.com/shuyra-arche/kernel/issues/333")[Copy] and
#link("https://github.com/shuyra-arche/kernel/issues/334")[Share and Activity]
remain part of that migration.
