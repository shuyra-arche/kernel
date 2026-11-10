// Shuyra Kernel design narrative.
// This file assembles problem-led chapters; it is not the public ABI reference.
#set document(
  title: [Shuyra Kernel — A Design for Coherence],
  author: ("Shuyra",),
  description: [The architectural motivation and safety arguments of Shuyra Kernel.],
)
#set page(paper: "a4", margin: (x: 2.5cm, y: 2.7cm), numbering: "1")
#set text(size: 10.5pt)
// Use the prose font family for inline terms in PDF previews.
#show raw.where(block: false): set text(font: "Libertinus Serif", weight: "semibold")
#set par(justify: true)
#set heading(numbering: "1.")

#align(center)[
  #text(size: 24pt, weight: "bold")[Shuyra Kernel]

  #v(0.5em)
  #text(size: 14pt)[A Design for Coherence]

  #v(1em)
  #text(size: 9pt)[Architectural draft]
]

#v(2em)
#outline(title: [Contents], depth: 2)
#pagebreak()

#include "motivation.typ"
#pagebreak(weak: true)
#include "composition.typ"
#pagebreak(weak: true)
#include "assurance.typ"
#pagebreak(weak: true)
#include "memory.typ"
