#let ink = rgb("#173D36")
#let muted = rgb("#465E64")
#let edge = rgb("#587678")
#let rule = rgb("#B9CBCA")
#let green = rgb("#E5F1EA")
#let amber = rgb("#F8EDDB")
#let pending = rgb("#8E6227")

#let at(x, y, body) = place(top + left, dx: x, dy: y, body)
#let drawing(height, body) = block(width: 100%, height: height, body)

#let label(x, y, body, color: muted) = at(x, y, text(
  size: 9pt, weight: "semibold", fill: color, body,
))

#let note(x, y, width, title, body, color: ink) = at(x, y, block(width: width)[
  #text(size: 11pt, weight: "semibold", fill: color, title) \
  #text(size: 10pt, body)
])

#let node(x, y, width, height, title, body, fill: white) = at(x, y, block(
  width: width,
  height: height,
  fill: fill,
  stroke: 0.65pt + rule,
  radius: 1.6mm,
  inset: (x: 3.8mm, y: 3.3mm),
)[
  #text(size: 11pt, weight: "semibold", fill: ink, title) \
  #text(size: 10pt, body)
])

#let wire(points, color: edge, dash: none) = {
  for (a, b) in points.zip(points.slice(1)) {
    let x = calc.min(a.at(0), b.at(0))
    let y = calc.min(a.at(1), b.at(1))
    at(x, y, line(
      start: (a.at(0) - x, a.at(1) - y),
      end: (b.at(0) - x, b.at(1) - y),
      stroke: (paint: color, thickness: 0.85pt, dash: dash),
    ))
  }
  let (x, y) = points.last()
  let (a, b) = points.at(-2)
  if x > a {
    at(x - 2mm, y - 0.95mm, polygon(
      (0mm, 0mm), (2mm, 0.95mm), (0mm, 1.9mm), fill: color,
    ))
  } else if x < a {
    at(x, y - 0.95mm, polygon(
      (2mm, 0mm), (0mm, 0.95mm), (2mm, 1.9mm), fill: color,
    ))
  } else if y > b {
    at(x - 0.95mm, y - 2mm, polygon(
      (0mm, 0mm), (1.9mm, 0mm), (0.95mm, 2mm), fill: color,
    ))
  }
}

#set par(justify: false)

= When can a page be reused?
<memory>

A worker maps one Page for device access, then exits with a write still
queued. Follow the holding, the mapping, and the work left for the Kernel.

== Who holds the Page?

#drawing(160mm)[
  #label(0mm, 1mm, [One Page, two relationships])
  #node(0mm, 14mm, 45mm, 29mm, [Slot in a Flat], [
    A materialized position in the logical backing.
  ])
  #node(57mm, 14mm, 45mm, 29mm, [Memo in a Nest], [
    An address position in the External Nest.
  ])
  #wire(((22mm, 43mm), (22mm, 76mm)))
  #wire(((79mm, 43mm), (79mm, 76mm)))
  #label(26mm, 57mm, [holds])
  #label(83mm, 57mm, [records])
  #node(11mm, 76mm, 88mm, 26mm, [Page], [
    A physical allocation with a distinct identity.
  ], fill: green)

  #wire(((102mm, 31mm), (112mm, 31mm)), dash: "dashed")
  #note(115mm, 24mm, 45mm, [The association persists], [
    If the Slot later changes, the Memo keeps its recorded Page until an
    explicit association transition.
  ])

  #at(0mm, 112mm, line(length: 160mm, stroke: 0.65pt + rule))
  #label(0mm, 117mm, [A reservation can come first])
  #node(0mm, 129mm, 54mm, 26mm, [Vacant], [
    A Slot-sourced Memo starts with an address reservation.
  ])
  #wire(((54mm, 142mm), (105mm, 142mm)))
  #label(64mm, 133mm, [Associate with Push])
  #node(105mm, 129mm, 55mm, 26mm, [Ready], [
    The address now has backing. Publication can follow.
  ], fill: green)
]

#pagebreak()

== When does the address become usable?

#drawing(173mm)[
  #wire(((157mm, 109mm), (157mm, 150mm), (1mm, 150mm), (1mm, 22mm), (7mm, 22mm)),
    color: pending, dash: "dashed")
  #node(7mm, 9mm, 51mm, 26mm, [Ready], [
    The Memo names its backing Page.
  ])
  #wire(((58mm, 22mm), (69mm, 22mm)), dash: "dashed")
  #note(73mm, 9mm, 79mm, [Who can ask for publication?], [
    The worker holds a delegated External Nest Pend and the required
    authority over the source.
  ])
  #wire(((32mm, 35mm), (32mm, 54mm)))
  #label(37mm, 42mm, [Publish])
  #node(7mm, 54mm, 51mm, 26mm, [Publishing], [
    The call can return while installation is in flight.
  ], fill: amber)

  #wire(((58mm, 70mm), (87mm, 70mm), (87mm, 85mm), (96mm, 85mm)), color: pending)
  #label(64mm, 62mm, [if it fails], color: pending)
  #node(96mm, 63mm, 64mm, 46mm, [Inspect again], [
    A failed publication can advance the Memo generation. Read the current
    state to see whether retry is allowed.
  ], fill: amber)

  #wire(((32mm, 80mm), (32mm, 110mm)))
  #label(37mm, 95mm, [Installation completes])
  #node(7mm, 110mm, 51mm, 28mm, [Published], [
    The access path to the same Page is installed.
  ], fill: green)
  #wire(((58mm, 124mm), (69mm, 124mm)), dash: "dashed")
  #note(73mm, 116mm, 79mm, [When the worker submits device work], [
    Inspect reports Published for the requested publication. The mapping
    and authority stay live throughout use.
  ])

  #label(24mm, 156mm, [Retry from Ready only with the current generation], color: pending)
]

#pagebreak()

== What survives the worker's exit?

#drawing(188mm)[
  #label(0mm, 0mm, [Task and authority])
  #label(87mm, 0mm, [Hardware and storage])
  #at(0mm, 8mm, line(length: 67mm, stroke: 0.65pt + rule))
  #at(87mm, 8mm, line(length: 73mm, stroke: 0.65pt + rule))

  #at(4mm, 17mm, circle(radius: 1.5mm, fill: edge, stroke: none))
  #note(13mm, 14mm, 54mm, [Task exits], [
    The manager can observe its result while retirement runs.
  ])
  #node(87mm, 14mm, 73mm, 28mm, [A write is still queued], [
    An earlier write can still attempt to use the mapping.
  ], fill: amber)

  #wire(((5.5mm, 20mm), (5.5mm, 55mm)))
  #wire(((123mm, 42mm), (123mm, 53mm)))
  #at(4mm, 55mm, circle(radius: 1.5mm, fill: edge, stroke: none))
  #note(13mm, 53mm, 54mm, [The Pend branch closes], [
    The departed holder loses its authority. The External Nest can remain
    bound.
  ])
  #wire(((67mm, 68mm), (87mm, 68mm)))
  #label(71mm, 60mm, [starts])
  #node(87mm, 53mm, 73mm, 31mm, [Withdraw the translation], [
    The Kernel starts hardware retirement and retains the records needed to
    finish it.
  ], fill: amber)

  #wire(((123mm, 84mm), (123mm, 95mm)))
  #wire(((123mm, 89mm), (77mm, 89mm), (77mm, 105mm), (68mm, 105mm)),
    color: pending, dash: "dashed")
  #note(1mm, 98mm, 65mm, [While evidence is missing], [
    The allocation stays retained. Task results and device notifications
    report progress. Settlement requires the hardware evidence.
  ], color: pending)
  #node(87mm, 95mm, 73mm, 34mm, [The old mapping settles], [
    The Kernel accepts matching IOMMU invalidation and fence completion
    together with trusted platform evidence.
  ], fill: green)

  #wire(((123mm, 129mm), (123mm, 139mm), (37mm, 139mm), (37mm, 151mm)))
  #wire(((123mm, 139mm), (123mm, 151mm)))
  #node(0mm, 151mm, 74mm, 33mm, [Address handover], [
    After the old positions are removed, a successor can use the same
    External Nest with a fresh Memo identity.
  ], fill: green)
  #node(86mm, 151mm, 74mm, 33mm, [Physical reuse], [
    Every CPU and device obligation must settle and every Page holder must
    release its claim.
  ], fill: green)
]

#text(size: 9pt)[
  The
  #link("https://github.com/shuyra-arche/kernel/blob/main/server/tests/guest/cell/external/exit.rs")[#underline[QEMU exit regression]]
  checks late-access denial and same-Nest handover. The
  #link("https://github.com/shuyra-arche/kernel/blob/main/api/README.md#cell-contracts")[#underline[Cell contract]]
  specifies the operations and their failure results.
]
