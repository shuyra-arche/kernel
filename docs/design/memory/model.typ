#import "diagram.typ": amber, drawing, green, label, node, note, rule, wire

== Cell and its coordinates

A Cell operation selects a position in a collection. The Server's `Cell`
enum gives that position its meaning through a Flat range or a Nest range.

#drawing(79mm)[
  #node(48mm, 2mm, 64mm, 23mm, [Cell], [Select a position in a range.])
  #wire(((80mm, 25mm), (80mm, 34mm), (37mm, 34mm), (37mm, 46mm)))
  #wire(((80mm, 34mm), (123mm, 34mm), (123mm, 46mm)))
  #node(0mm, 46mm, 74mm, 28mm, [Flat range], [
    A Slot selects the current physical holding at a backing position.
  ])
  #node(86mm, 46mm, 74mm, 28mm, [Nest range], [
    A Memo records an address position and, when associated, the Page it names.
  ], fill: green)
]

The public `CellObject` selects a Slot handle or a mapping target and
identity. `CellPosition` adds a relative offset and, for a Memo, an expected
generation. A Slot position uses generation zero and refers to the current
holding. A managed `Page` has its own identity and the allocator's unique
return receipt. Several records can retain that same Page.

#table(
  columns: (38mm, 1fr),
  stroke: 0.5pt + rule,
  inset: 7pt,
  [*Quantity*], [*Meaning*],
  [ABI coordinate], [Offsets and spans count fixed 4 KiB units.],
  [Physical allocation], [`CellLevel` sets the size and alignment of a contiguous allocation to #box[4 KiB × 512#super[level]]. Operations address the head and cover the whole Cell.],
  [Translation leaf], [The backend selects a leaf geometry. One large CPU leaf can cover several logical positions with distinct identities and obligations.],
)

For example, a level-1 allocation spans 512 ABI coordinates and needs an
aligned 2 MiB physical range. Admission also checks the active Sv mode,
and allocation depends on available RAM.

#text(size: 9pt)[
  #link("https://github.com/shuyra-arche/kernel/blob/main/server/src/basis/cell/mod.rs")[#underline[Server Cell]]
  · #link("https://github.com/shuyra-arche/kernel/blob/main/api/README.md#memory-geometry-three-distinct-units")[#underline[ABI geometry]]
]

#pagebreak()

== Flat and the unique holding

A Flat lays out logical backing positions. Managed positions materialize
allocator-owned Pages on demand. A borrowed Flat describes an externally
maintained physical range, such as MMIO, with its own lifetime contract.

#drawing(48mm)[
  #node(0mm, 4mm, 76mm, 37mm, [Fresh · absent from the Slot table], [
    This position has yet to materialize. A request can allocate zeroed
    backing for it.
  ])
  #node(84mm, 4mm, 76mm, 37mm, [Missing · explicit Slot state], [
    Its Page was moved out. Only an incoming handoff can give the position
    another holding.
  ], fill: amber)
]

That distinction prevents a later access from silently filling a hole
created by a transfer. During a handoff, the two positions carry the same
transaction identity.

#drawing(95mm)[
  #label(0mm, 1mm, [Source])
  #node(0mm, 11mm, 45mm, 25mm, [Resident], [Holds Page P.])
  #wire(((45mm, 23mm), (57mm, 23mm)))
  #node(57mm, 11mm, 46mm, 25mm, [Moving], [Retains P and the transaction ID.], fill: amber)
  #wire(((103mm, 23mm), (115mm, 23mm)))
  #node(115mm, 11mm, 45mm, 25mm, [Missing], [Records the move.])

  #label(0mm, 47mm, [Destination])
  #node(0mm, 57mm, 45mm, 26mm, [Fresh or Missing], [Eligible empty position.])
  #wire(((45mm, 69mm), (57mm, 69mm)))
  #node(57mm, 57mm, 46mm, 26mm, [Reserved], [Remembers the ID and prior state.], fill: amber)
  #wire(((103mm, 69mm), (115mm, 69mm)))
  #node(115mm, 57mm, 45mm, 26mm, [Resident], [Holds the same Page P.], fill: green)
]

The transaction prepares both ends before claiming either Slot. While it
is in flight, ordinary materialization and publication are excluded. Commit
moves the existing holding with both Flat states locked, or with one lock
when the positions share a Flat. Dropping the uncommitted guard restores
both prior states, including
whether the destination was Fresh or Missing. Existing Memo associations
keep their recorded Page.

A Slot obtained through Flat Slice retains its exact Pend branch. Each use
checks the live window and authority again. Deleting that Slot handle releases
the handle. Deleting the Flat seals the source and starts withdrawal of
its associations.

#text(size: 9pt)[
  #link("https://github.com/shuyra-arche/kernel/blob/main/server/src/basis/flat/storage/slot.rs")[#underline[Slot state]]
  · #link("https://github.com/shuyra-arche/kernel/blob/main/server/src/basis/flat/transaction.rs")[#underline[Handoff transaction]]
]

#pagebreak()

== Nest and the address lifetime

A Nest owns address occupancy and the records that turn addresses into
access paths. An Internal Nest uses CPU translation. Trusted boot admits
an External Nest for an authenticated device requester and gives its
manager the root Pend.

#drawing(155mm)[
  #node(0mm, 4mm, 53mm, 27mm, [Reserve], [
    Occupancy records the address interval.
  ])
  #wire(((53mm, 17mm), (64mm, 17mm)), dash: "dashed")
  #note(68mm, 4mm, 92mm, [Who can take this address?], [
    Fixed and dynamic placement share one occupancy record. A retiring
    interval stays occupied until its ticket is settled.
  ])

  #wire(((26mm, 31mm), (26mm, 43mm)))
  #node(0mm, 43mm, 53mm, 27mm, [Associate], [
    A Memo records the selected Page.
  ])
  #wire(((53mm, 56mm), (64mm, 56mm)), dash: "dashed")
  #note(68mm, 43mm, 92mm, [What does the position refer to?], [
    A MappingId and relative offset select the position. Its generation
    qualifies the transition. Reassociation changes the Page while the
    position keeps its identity.
  ])

  #wire(((26mm, 70mm), (26mm, 82mm)))
  #node(0mm, 82mm, 53mm, 27mm, [Publish], [
    Create the access path to the associated Page.
  ], fill: green)
  #wire(((53mm, 95mm), (64mm, 95mm)), dash: "dashed")
  #note(68mm, 82mm, 92mm, [What can reach the Page?], [
    The relevant MMU or IOMMU realizes access. The Kernel records the
    publication obligation before hardware can observe it.
  ])

  #wire(((26mm, 109mm), (26mm, 121mm)))
  #node(0mm, 121mm, 53mm, 29mm, [Retract], [
    Withdraw access and settle earlier effects.
  ], fill: amber)
  #wire(((53mm, 135mm), (64mm, 135mm)), dash: "dashed")
  #note(68mm, 121mm, 92mm, [What can be kept or removed?], [
    A settled Memo can keep its association for another publication.
    Removing the old position also releases its address claim once
    retirement is complete.
  ])
]

A worker's Pend branch can retire while the External Nest remains bound.
This lets a manager hand the same address space to a successor after the
old positions are settled and removed.

#text(size: 9pt)[
  #link("https://github.com/shuyra-arche/kernel/blob/main/server/src/basis/nest/address/occupancy.rs")[#underline[Occupancy]]
  · #link("https://github.com/shuyra-arche/kernel/blob/main/api/README.md#runtime-organizations")[#underline[Nest contract]]
]

#pagebreak()

== What the state model must remember

A position, a hardware publication, and the physical allocation can finish
at different times. The code keeps the facts needed to decide each of those
transitions with its responsible object.

#table(
  columns: (38mm, 1fr),
  stroke: 0.5pt + rule,
  inset: 7pt,
  [*Record*], [*Facts it carries*],
  [`Memo`], [The association, generation, optional freeze and in-flight `MemoFlight`. `Inspect` derives Vacant, Ready, Publishing, Published, Retracting or Failed from these facts and the publication phase. A retracted Memo can be Ready and still frozen.],
  [`Publication`], [The per-publication hardware obligation. `Waiting(BatchId)` remembers which batch is pending. `Failed` preserves an unresolved obligation, so failure still leaves work for retirement.],
  [`Page`], [The physical allocation, owner and reclamation state. The state tracks CPU and external uses, write exclusivity, an active change gate and quarantine.],
)

#drawing(53mm)[
  #label(0mm, 3mm, [One hardware use represented by Retention])
  #node(0mm, 16mm, 36mm, 28mm, [Prepared], [Use reserved.])
  #wire(((36mm, 29mm), (42mm, 29mm)))
  #node(42mm, 16mm, 36mm, 28mm, [Published], [Before first visible effect.])
  #wire(((78mm, 29mm), (84mm, 29mm)))
  #node(84mm, 16mm, 36mm, 28mm, [Retiring], [Evidence still owed.], fill: amber)
  #wire(((120mm, 29mm), (126mm, 29mm)))
  #node(126mm, 16mm, 34mm, 28mm, [Settled], [Use discharged.], fill: green)
]

In this row, Published means the retention has committed its obligation.
A Memo reports Published when its access path is installed. A prepared
use can be cancelled. If a published or retiring record is
lost, its drop path quarantines the Page and retains the allocation.
`State::transition` computes the next reclamation state from an event.
The atomic commit retries against a newer state if another update wins.
For example, when a change closes ordinary admission first, a racing
registration rechecks the gate and returns Busy.

Physical return happens when the last holder releases the Page and its
reclamation state permits return. A Team ownership epoch can close earlier,
leaving the Kernel responsible for the outstanding allocation and access.

#text(size: 9pt)[
  #link("https://github.com/shuyra-arche/kernel/blob/main/server/src/basis/nest/memo/mod.rs")[#underline[Memo]]
  · #link("https://github.com/shuyra-arche/kernel/blob/main/server/src/basis/nest/memo/publication/receipt.rs")[#underline[Publication]]
  · #link("https://github.com/shuyra-arche/kernel/blob/main/server/src/basis/cell/page/reclaim/mod.rs")[#underline[Page reclamation]]
  · #link("https://github.com/shuyra-arche/kernel/blob/main/server/src/basis/cell/page/reclaim/retention.rs")[#underline[Retention]]
]
