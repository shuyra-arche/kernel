#import "../style.typ": amber, design-table, edge, green, ink, pending, principle, rule
#import "diagram.typ": drawing, label, node, wire

// A local visual vocabulary for arguments, not an additional Kernel model.
// Keep the failure next to the invariant that rules it out.
#let contrast(problem, invariant) = grid(
  columns: (1fr, 1fr),
  column-gutter: 4mm,
  block(
    fill: amber,
    radius: 1.8mm,
    inset: (x: 3.5mm, y: 3mm),
  )[
    #text(size: 8pt, weight: "bold", fill: pending)[FAILURE MODE]
    #v(2pt)
    #problem
  ],
  block(
    fill: green,
    radius: 1.8mm,
    inset: (x: 3.5mm, y: 3mm),
  )[
    #text(size: 8pt, weight: "bold", fill: ink)[REQUIRED INVARIANT]
    #v(2pt)
    #invariant
  ],
)

#let source(url, title) = text(size: 9pt)[
  #link(url)[#underline[#title]]
]

== Why a Page cannot have one lifetime

Imagine a Page that a user-space pager may move, a CPU task may address,
and an external requester may access through an IOMMU. Revoking the task's
permission need not finish the device's earlier work. Moving the backing
holding need not remove an existing address association. Deleting an address
record need not mean that the underlying allocation can be returned.

#contrast(
  [If all of those actions are treated as one "unmap and free" operation,
   a stale translation or queued write can reach a reallocated Page.],
  [Physical holding, logical association, and hardware access are
   independently represented. Reuse waits for the obligations belonging
   to each relationship.],
)

#drawing(75mm)[
  #label(0mm, 0mm, [Three relationships; one allocation])
  #node(0mm, 10mm, 48mm, 27mm, [Flat / Slot], [
    Unique owning holding.
  ], fill: green)
  #node(56mm, 10mm, 48mm, 27mm, [Nest / Memo], [
    Address association.
  ])
  #node(112mm, 10mm, 48mm, 27mm, [Publication], [
    Hardware responsibility.
  ], fill: amber)
  #wire(((24mm, 37mm), (24mm, 46mm), (74mm, 46mm), (74mm, 51mm)))
  #wire(((80mm, 37mm), (80mm, 51mm)))
  #wire(((136mm, 37mm), (136mm, 46mm), (86mm, 46mm), (86mm, 51mm)))
  #node(56mm, 51mm, 48mm, 24mm, [Page P], [
    Physical allocation.
  ], fill: green)
]

A `Slot` owns the unique holding; a `Memo` can retain a reference to
the same `Page` without becoming a second owning Slot. A `Publication`
retains the obligation created when an MMU or IOMMU path is made visible.
`Cell` is the common way to select positions in these organizations,
not another physical owner.

#principle[
  The object that names a Page, the object that owns its holding, and
  the record that proves old access has ended must not be confused.
]

== Cell: selecting a position is not owning a Page

A physical address alone cannot select an unmapped holding for handoff.
A virtual address alone cannot describe a holding after its address has
been removed. Nor may a stale mapping number silently act on a new
association at the same position.

#contrast(
  [A raw address or reusable number carries too little information to
   distinguish the intended relationship and its current revision.],
  [A Cell operation selects either a Slot position in Flat or a Memo
   position in Nest. The operation checks the appropriate live authority
   and, for Memo, its expected generation.],
)

The Server's `Cell` has the two variants `Flat(FlatRange)` and
`Nest(NestRange)`. At the ABI boundary, `CellObject` instead selects a
Task-local `SlotId` or a live `Memo` `PendId`. `MemoId` and
`MappingId` may identify objects for observation, but do not grant
permission to operate on them.

#design-table(
  table.header([*Coordinate*], [*Meaning and rule*]),
  [`CellObject::Slot`], [Selects the current unique holding through a Task-local Slot. It is not a historical content snapshot.],
  [`CellObject::Memo`], [Selects the mapping through a live Pend. A supplied descriptive ID cannot replace that authority.],
  [`CellPosition`], [Adds a relative position and a generation. Slot uses generation zero; Memo operations reject stale generations.],
)

A logical ABI coordinate is a fixed 4 KiB unit, *not* a promise that
physical Cells or translation leaves are all 4 KiB. `CellLevel` chooses
contiguous physical geometry of $4096 times 512^("level")$ bytes, subject
to the active Sv mode and physical capacity. Hardware backends choose
their own translation leaves. In the current implementation External
Memos admit complete level-0 or level-1 Cells, with 4 KiB IOMMU leaves.

#text(size: 9pt)[
  Implementation: #source("https://github.com/shuyra-arche/kernel/blob/main/server/src/basis/cell/mod.rs", [Cell])
  · #source("https://github.com/shuyra-arche/kernel/blob/main/api/src/model/cell/object.rs", [CellPosition])
  · #source("https://github.com/shuyra-arche/kernel/blob/main/api/src/model/cell/level.rs", [CellLevel])
]

== Flat: a moved-out holding is not fresh storage

Consider a pager that moves a resident Page to another owning Slot.
The original address position may remain reserved, but its physical
holding is no longer there. If a later fault treats that absence as
never-allocated memory and fills it with zeros, it silently loses the
original contents.

#contrast(
  [Conflating "never materialized" with "deliberately moved out"
   makes a racing fault capable of replacing application data.],
  [An absent entry is `Fresh` and may materialize; an explicit
   `Missing` entry is a tombstone and cannot auto-materialize.],
)

#drawing(92mm)[
  #label(0mm, 0mm, [One handoff: source and destination change together])
  #label(0mm, 13mm, [Source Slot])
  #node(0mm, 24mm, 44mm, 27mm, [Resident], [Holds P.], fill: green)
  #wire(((44mm, 37mm), (58mm, 37mm)))
  #node(58mm, 24mm, 44mm, 27mm, [Moving], [P + transaction.], fill: amber)
  #wire(((102mm, 37mm), (116mm, 37mm)))
  #node(116mm, 24mm, 44mm, 27mm, [Missing], [No implicit refill.])
  #label(0mm, 56mm, [Destination Slot])
  #node(0mm, 66mm, 44mm, 26mm, [Fresh / Missing], [Eligible position.])
  #wire(((44mm, 79mm), (58mm, 79mm)))
  #node(58mm, 66mm, 44mm, 26mm, [Reserved], [Transaction + origin.], fill: amber)
  #wire(((102mm, 79mm), (116mm, 79mm)))
  #node(116mm, 66mm, 44mm, 26mm, [Resident], [Now holds P.], fill: green)
]

These are not merely presentation labels. The source's `Moving` state
retains the original Page and the transaction identity; the destination's
`Reserved` state remembers whether it was `Fresh` or `Missing`. The
operation prepares both endpoints before mutating either Slot and commits
under the relevant Flat lock or ordered lock pair. An uncommitted guard
restores both prior states.

A `Push` moves `local -> remote` and a `Pull` moves
`remote -> local`. Both endpoints must be owning Slots, with separate
live transfer authorization. Neither operation copies the Page, changes
the current Memo association, nor edits PTEs.

#design-table(
  table.header([*Operation*], [*Relationship it changes*]),
  [`Push / Pull`], [Transfer the unique Page holding from one owning Slot to another. A Memo is never a handoff endpoint.],
  [`Insert / Remove`], [Create or clear a Memo-to-Page association, with source checks and current Memo generation. The owning Slot does not move.],
  [`Publish / Retract`], [Establish or withdraw the hardware access path for an associated Page. The hardware obligation survives until settlement.],
)

This separation makes a user-space Page Service possible without giving
it unconditional ownership over every client's memory. Flat TRANSFER
must authorize both Slot endpoints when applicable; Memo authority cannot
be converted into an owning Slot. A transfer also requires a movable
Page, so an outstanding hardware obligation is not bypassed by moving
the handle.

#text(size: 9pt)[
  Implementation: #source("https://github.com/shuyra-arche/kernel/blob/main/server/src/basis/flat/storage/slot.rs", [Slot states])
  · #source("https://github.com/shuyra-arche/kernel/blob/main/server/src/basis/flat/transaction.rs", [Transaction and rollback])
]

== Nest: an association is not a publication

Suppose an address is reserved before backing exists, or a Page's Slot
changes while an existing mapping still retains it. Making the virtual
address and physical ownership one object would force those independent
lifetimes to change together. Conversely, allowing reuse of an old
address before hardware withdrawal settles risks reaching the successor
through a stale translation.

#contrast(
  [Reusing an address immediately after a request to unmap it confuses
   a software decision with completion on every possible initiator.],
  [Nest retains address occupancy and Memo identity until the old
   publication and its hardware retirement obligations have settled.],
)

A `Nest` owns address occupancy and its `Memo` records. `Internal`
uses the CPU MMU; `External` represents an authenticated requester
admitted at trusted boot and enforced by an IOMMU backend. They share
the association and retirement vocabulary, *not* identical hardware
instructions, placement limits, or concurrency protocols.

#design-table(
  table.header([*Memo observation*], [*Meaning for the caller*]),
  [`Vacant`], [Address position exists, but no Page is associated. Insert may supply backing with appropriate authority.],
  [`Ready`], [The Memo names a Page; publication is not yet established. Ready may also retain a freeze after withdrawal.],
  [`Publishing / Published`], [Installation is in flight or the access path is installed. Caller success and hardware completion are distinct.],
  [`Retracting / Failed`], [Withdrawal is in flight, or an unresolved hardware failure remains. Neither state licenses physical reuse.],
)

An `Insert` can associate a Page without transferring its Slot.
`Publish` produces a hardware obligation; `Retract` withdraws that
specific publication. `Remove` can clear the association only after
publication has settled. Each Memo transition checks and may advance
a non-wrapping generation. After `Busy` or a failed publication,
`Inspect` must be used to recover the current state and generation;
replaying a stale request never starts a new cycle.

Delegating a Nest or Memo `Pend` grants a bounded operation set,
not a raw hardware credential. A worker's branch can retire and its
Memos can be cleaned up while a manager retains the parent External
Nest. The manager can reuse settled address positions with new Memo
identities, without pretending that the worker's exit settled earlier
device operations.

#text(size: 9pt)[
  Implementation: #source("https://github.com/shuyra-arche/kernel/blob/main/server/src/basis/nest/memo/mod.rs", [Memo generation and flights])
  · #source("https://github.com/shuyra-arche/kernel/blob/main/server/src/basis/nest/address/occupancy.rs", [Address occupancy])
]

== Page: access responsibility is not a reference count

A software reference can keep a Page allocated, but cannot alone show
that a CPU TLB entry or IOMMU request has stopped using it. The Kernel
therefore retains per-publication access obligations until the
appropriate backend proves completion.

#drawing(60mm)[
  #label(0mm, 1mm, [A Retention's obligation, not a syscall state])
  #node(0mm, 14mm, 36mm, 30mm, [Prepared], [Can cancel.])
  #wire(((36mm, 29mm), (42mm, 29mm)))
  #node(42mm, 14mm, 36mm, 30mm, [Published], [Effect possible.])
  #wire(((78mm, 29mm), (84mm, 29mm)))
  #node(84mm, 14mm, 36mm, 30mm, [Retiring], [Evidence owed.], fill: amber)
  #wire(((120mm, 29mm), (126mm, 29mm)))
  #node(126mm, 14mm, 34mm, 30mm, [Settled], [Use discharged.], fill: green)
]

`Retention::publish` commits responsibility *before* the first possibly
visible hardware effect. `settle` is permitted only with proof that
the exact translation and in-flight accesses have ended. Dropping a
published or retiring Retention without that proof quarantines the
allocation instead of falsely freeing it. A failed withdrawal remains
a live cleanup problem.

The Page reclamation code demonstrates an explicit state-and-effect
boundary. `State::transition(self, event)` computes a prospective state;
the atomic compare-exchange retries from the observed state if another
update wins. A concurrent registration therefore rechecks the same
admission gate as the attempted change, rather than trusting a stale
permission snapshot.

#raw("let after = State::decode(before).transition(event)?.encode();\nmatch self.0.compare_exchange_weak(\n    before, after, Ordering::AcqRel, Ordering::Acquire\n) {\n    Ok(_) => return Ok(()),\n    Err(current) => before = current,\n}", block: true, lang: "rust")

This is a local safety policy, not a promise that every access protocol
is supported. For the *same Page*, current registration allows CPU read
with External read; it rejects CPU write with External read, CPU read
with External write, and simultaneous CPU and External writes. In
particular, coherent, concurrently writable device rings cannot be
implemented merely by relaxing this conflict test: they require an
explicitly verified synchronization and ownership protocol.

#principle[
  Closing admission, withdrawing access, proving prior effects settled,
  and finally reclaiming storage are different obligations. A Task exit,
  a Sign notification, or an operation completion is not automatically
  hardware-retirement evidence.
]

#text(size: 9pt)[
  Implementation: #source("https://github.com/shuyra-arche/kernel/blob/main/server/src/basis/cell/page/reclaim/mod.rs", [Pure transition and atomic commit])
  · #source("https://github.com/shuyra-arche/kernel/blob/main/server/src/basis/cell/page/reclaim/retention.rs", [Retention])
  · #source("https://github.com/shuyra-arche/kernel/blob/main/server/src/basis/nest/memo/publication/receipt.rs", [Publication])
]

== A combined failure: worker exit during device access

The next worked example follows an External Memo while a worker exits
with an earlier device write still queued. Read it as four distinct
questions, not as a promise that a single call performs them all.

#design-table(
  table.header([*Question*], [*Enforcing boundary*]),
  [Who may act now?], [The worker's Pend branch closes. It cannot authorize another publication; the manager may still hold the Nest root.],
  [What address is reserved?], [Nest retains the affected positions while retirement is outstanding. An old Memo identity does not authorize its successor.],
  [Can hardware still touch P?], [The Publication / Retention remains until the exact IOMMU invalidation, ordering, and trusted platform completion evidence is accepted.],
  [Can P be reused?], [Only after every CPU and External obligation settles and all remaining holders release the Page may the physical allocator receive it.],
)

The public memory SnapshotPage exposes sampled aggregate pressure such as
`cpu_retained`, `io_retained`, `retiring`, and `abandoned` through a
read-only Share and Sign. It does not export physical addresses, prove
that an individual Page is reclaimable, or implement a page-replacement
policy. In the current preview `accessed` and `dirty` remain
`UNKNOWN`; per-Page A/D sampling is not supplied by this observation.

The diagrams and state transitions above are architectural explanations,
not full verification evidence. The Slot handoff contract needs host
state and concurrency checks; CPU publication and External retirement
need target or QEMU scenarios; physical requester quiescence needs
evidence from the actual trusted platform. The following sections show
the device-exit flow, while the API remains the source of exact call
permissions and supported geometry.

#text(size: 9pt)[
  Contracts: #source("https://github.com/shuyra-arche/kernel/blob/main/api/README.md#cell-contracts", [Cell calls])
  · #source("https://github.com/shuyra-arche/kernel/blob/main/api/README.md#global-memory-observations", [Memory observations])
  · #source("https://github.com/shuyra-arche/kernel/blob/main/server/tests/guest/cell/external/exit.rs", [External-exit regression])
]
