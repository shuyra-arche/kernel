#let ink = rgb("#172B35")
#let secondary = rgb("#46606B")
#let stroke-color = rgb("#78909A")
#let paper = rgb("#F1F5F5")
#let border = rgb("#CCD8DA")
#let page-fill = rgb("#D9ECE7")
#let pending-fill = rgb("#F3E7CC")

#let at(x, y, body) = place(top + left, dx: x, dy: y, body)

#let card(x, y, width, title, detail, fill: white) = at(x, y, block(
  width: width,
  height: 23mm,
  fill: fill,
  stroke: 0.65pt + border,
  radius: 2mm,
  inset: (x: 4mm, y: 3.5mm),
)[
  #set par(justify: false)
  #text(size: 11pt, weight: "semibold", fill: ink, title) \
  #text(size: 9pt, fill: secondary, detail)
])

#let arrow(x, y, right: 0mm, down: 0mm) = {
  at(x, y, line(end: (right, down), stroke: 0.8pt + stroke-color))
  if right > 0mm {
    at(x + right - 1mm, y - 0.85mm, polygon(
      (0mm, 0mm), (1.4mm, 0.85mm), (0mm, 1.7mm), fill: stroke-color,
    ))
  } else {
    at(x - 0.85mm, y + down - 1mm, polygon(
      (0mm, 0mm), (1.7mm, 0mm), (0.85mm, 1.4mm), fill: stroke-color,
    ))
  }
}

#let label(x, y, body) = at(x, y, text(size: 9pt, fill: secondary, body))

#let panel(tag, caption, body) = figure(
  block(
    width: 100%,
    height: 103mm,
    fill: paper,
    stroke: 0.65pt + border,
    radius: 3mm,
  )[
    #at(8mm, 8mm, text(size: 10pt, weight: "semibold", fill: secondary, tracking: 0.6pt, tag))
    #at(8mm, 17mm, line(length: 144mm, stroke: 0.65pt + border))
    #body
  ],
  kind: image,
  caption: caption,
)

#let takeaway(body) = block(
  width: 100%,
  fill: rgb("#EEF5F2"),
  inset: (x: 5mm, y: 3mm),
  radius: 1.5mm,
)[#set par(justify: false); #body]

= When can a page be reused?
<memory>

Follow one physical Page held in a Flat. A worker associates it with an
External Nest, publishes it for device access, and exits with a write queued.

== Who holds the Page?

#panel([01 / STORAGE AND ASSOCIATION],
  [The Slot retains the holding when the worker associates the Memo.],
  [
    #card(8mm, 27mm, 69mm, [Slot], [A position in a Flat])
    #card(83mm, 27mm, 69mm, [Memo], [A position in a Nest])
    #arrow(28mm, 50mm, down: 20mm)
    #arrow(103mm, 50mm, down: 20mm)
    #label(32mm, 56mm, [holds the Page])
    #label(107mm, 56mm, [associates the Page])
    #card(8mm, 70mm, 144mm, [Page], [One physical allocation with its own identity], fill: page-fill)
  ],
)

The worker uses `Push` from a materialized Slot to a Memo. The association
records the Page at that moment. If a later operation changes the Slot, the
Memo keeps referring to its recorded Page until an explicit association
transition.

#takeaway[*An address can be reserved before it has backing.* A Slot-sourced
Memo starts Vacant. Associating the Page moves the Memo to Ready.]

#pagebreak()

== When does the address become usable?

The worker holds a delegated External Nest Pend and permission over the
source. With the Memo Ready, the worker asks the Kernel to publish it.

#panel([02 / PUBLICATION],
  [External publication can remain in flight after Publish returns.],
  [
    #card(8mm, 32mm, 44mm, [Ready], [Page associated])
    #card(58mm, 32mm, 44mm, [Publishing], [Installation in flight], fill: pending-fill)
    #card(108mm, 32mm, 44mm, [Published], [Access path installed], fill: page-fill)
    #arrow(52.5mm, 43.5mm, right: 4.5mm)
    #arrow(102.5mm, 43.5mm, right: 4.5mm)
    #label(8mm, 24mm, [Before Publish])
    #label(108mm, 24mm, [After completion])
    #card(8mm, 70mm, 144mm, [Same Page throughout], [Publication changes addressability.], fill: page-fill)
  ],
)

The worker waits for `Inspect` to report Published for the requested
publication, then submits device work while the mapping and its authority
remain live.

#takeaway[*Retry from a fresh observation.* A failed publication can advance
the Memo generation. Inspect the current state before retrying.]

#pagebreak()

== What survives the worker's exit?

The worker exits with a write queued. Its manager can observe the Task
result while retirement of the old Memo is still in progress.

#panel([03 / RETIREMENT AND REUSE],
  [Task exit closes a branch. Hardware settlement and release of all Page holders gate reuse.],
  [
    #card(8mm, 31mm, 44mm, [Task exits], [Old branch closes])
    #card(58mm, 31mm, 44mm, [Retirement], [Settle former access], fill: pending-fill)
    #card(108mm, 31mm, 44mm, [Reuse gate], [All access settled \ All holders released], fill: page-fill)
    #arrow(52.5mm, 42.5mm, right: 4.5mm)
    #arrow(102.5mm, 42.5mm, right: 4.5mm)
    #card(8mm, 70mm, 94mm, [Page retained], [The Kernel keeps the allocation through retirement.], fill: pending-fill)
    #card(108mm, 70mm, 44mm, [Eligible for reuse], [Physical allocation], fill: page-fill)
  ],
)

The Kernel withdraws the translation and waits for matching IOMMU
invalidation and fence completion, together with trusted platform evidence.
If that evidence is missing, the allocation stays retained. Completion
settles this mapping. Physical reuse also requires every other CPU or
device obligation to settle and all Page holders to release their claims.

#takeaway[*The External Nest can outlive the worker.* Once its old positions
are retired and removed, a successor can receive authority for the same
Nest and reserve the former address with a fresh Memo identity.]

#text(size: 9pt)[
  See the
  #link("https://github.com/shuyra-arche/kernel/blob/main/api/README.md#cell-contracts")[#underline[Cell contract]]
  and the
  #link("https://github.com/shuyra-arche/kernel/blob/main/server/tests/guest/cell/external/exit.rs")[#underline[QEMU exit regression]].
  The regression covers late-access denial and same-Nest handover.
]
