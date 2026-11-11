#let text-ink = rgb("#23302E")
#let ink = rgb("#17463C")
#let muted = rgb("#47665D")
#let edge = rgb("#50796D")
#let rule = rgb("#C2D3CA")
#let green = rgb("#E1F0E7")
#let amber = rgb("#F6E9D7")
#let pending = rgb("#8B5724")
#let cover-ink = rgb("#113B33")
#let cover-light = rgb("#C1DBCD")
#let paper = rgb("#FEFDFB")

#let digits(n) = if n < 10 { "0" + str(n) } else { str(n) }

#let eyebrow(body, color: muted) = text(
  size: 8pt,
  weight: "semibold",
  tracking: 0.85pt,
  fill: color,
  body,
)

#let running-head = context {
  let number = here().page()
  if number > 1 {
    let chapters = query(heading.where(level: 1)).filter(h => h.location().page() <= number)
    let title = if chapters.len() > 0 { chapters.last().body } else { [Contents] }
    grid(
      columns: (auto, 1fr),
      column-gutter: 7mm,
      eyebrow([SHUYRA / KERNEL DESIGN]),
      align(right, text(size: 8.5pt, fill: muted, title)),
    )
    v(1.8mm)
    line(length: 100%, stroke: 0.55pt + rule)
  }
}

#let running-foot = context {
  let number = here().page()
  if number > 1 {
    line(length: 100%, stroke: 0.55pt + rule)
    v(1.8mm)
    grid(
      columns: (1fr, auto),
      eyebrow([ARCHITECTURAL DRAFT]),
      text(size: 11pt, weight: "semibold", fill: ink, digits(number)),
    )
  }
}

#let chapter-title(h) = block(above: 0pt, below: 9pt, sticky: true)[
  #grid(
    columns: (13mm, 1fr),
    column-gutter: 3.5mm,
    align(top, text(
      size: 22pt,
      weight: "semibold",
      fill: edge,
      context digits(counter(heading).get().first()),
    )),
    align(top, text(size: 22pt, weight: "bold", fill: ink, h.body)),
  )
  #v(1.8mm)
  #line(length: 100%, stroke: 0.8pt + rule)
]

#let section-title(h) = block(above: 14pt, below: 7pt, sticky: true)[
  #grid(
    columns: (auto, 1fr),
    column-gutter: 2mm,
    text(size: 12pt, weight: "semibold", fill: edge, context counter(heading).display(h.numbering)),
    text(size: 13pt, weight: "bold", fill: ink, h.body),
  )
]

#let editorial(body) = {
  set page(
    paper: "a4",
    fill: paper,
    margin: (x: 2.5cm, y: 2.7cm),
    header: running-head,
    footer: running-foot,
  )
  set text(font: "Libertinus Serif", size: 10.5pt, fill: text-ink)
  set par(justify: true)
  set heading(numbering: "1.")
  // Match prose fonts for inline terms in PDF previews.
  show raw.where(block: false): set text(font: "Libertinus Serif", weight: "semibold")
  show heading.where(level: 1): chapter-title
  show heading.where(level: 2): section-title
  show outline.entry.where(level: 1): set text(weight: "semibold", fill: ink)
  show outline.entry.where(level: 2): set text(size: 9.7pt, fill: muted)
  body
}

#let cover-mark = box(width: 55mm, height: 53mm)[
  #place(center + horizon, circle(radius: 24mm, stroke: 0.55pt + rgb("#55877A")))
  #place(center + horizon, dx: -3mm, dy: 2mm, circle(radius: 17mm, stroke: 0.65pt + rgb("#79A596")))
  #place(center + horizon, dx: 3mm, dy: 2mm, circle(radius: 9mm, stroke: 0.8pt + cover-light))
  #place(center + horizon, dx: 3mm, dy: 2mm, circle(radius: 2.2mm, fill: cover-light, stroke: none))
  #place(top + left, dx: 3.5mm, dy: 28.5mm, line(length: 48mm, stroke: 0.55pt + rgb("#55877A")))
  #place(top + left, dx: 39.5mm, dy: 9mm, circle(radius: 1.1mm, fill: rgb("#A7C8B8"), stroke: none))
]

#let cover = block(
  width: 100%,
  height: 65mm,
  fill: cover-ink,
  radius: 2.5mm,
  inset: (x: 9mm, y: 8mm),
  clip: true,
)[
  #place(top + right, dx: 2.5mm, dy: -1.5mm, cover-mark)
  #place(top + left, eyebrow([SHUYRA / DESIGN DOCUMENT], color: cover-light))
  #place(top + left, dy: 16mm, text(size: 32pt, weight: "bold", fill: white)[Shuyra Kernel])
  #place(top + left, dy: 31mm, text(size: 16.5pt, fill: cover-light)[A Design for Coherence])
  #place(top + left, dy: 44mm, eyebrow([ARCHITECTURAL DRAFT], color: cover-light))
]

#let principle(body) = block(
  width: 100%,
  fill: rgb("#EDF5EF"),
  stroke: (left: 1.8pt + edge),
  inset: (x: 4mm, y: 2.6mm),
  above: 7pt,
  below: 7pt,
)[
  #set par(justify: false)
  #body
]

#let design-table(..cells) = table(
  columns: (38mm, 1fr),
  stroke: (x, y) => (bottom: 0.55pt + rule),
  fill: (x, y) => if y == 0 { green } else if calc.even(y) { rgb("#F4F8F4") } else { paper },
  inset: (x: 8pt, y: 7pt),
  ..cells,
)
