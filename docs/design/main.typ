// Shuyra Kernel design narrative.
// This file assembles problem-led chapters; it is not the public ABI reference.
#set document(
  title: [Shuyra Kernel — A Design for Coherence],
  author: ("Shuyra",),
  description: [The architectural motivation and safety arguments of Shuyra Kernel.],
)
#import "style.typ": cover, editorial, eyebrow, rule
#show: editorial

#cover
#v(6mm)
#eyebrow([CONTENTS])
#v(1.5mm)
#line(length: 100%, stroke: 0.55pt + rule)
#outline(title: none, depth: 2)
#pagebreak()

#include "motivation.typ"
#pagebreak(weak: true)
#include "composition.typ"
#pagebreak(weak: true)
#include "assurance.typ"
#pagebreak(weak: true)
#include "memory.typ"
