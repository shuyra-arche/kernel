#import "../style.typ": amber, edge, green, ink, muted, pending, rule

#let at(x, y, body) = place(top + left, dx: x, dy: y, body)
#let drawing(height, body) = block(width: 100%, height: height, body)

#let label(x, y, body, color: muted) = at(x, y, text(
  size: 9pt, weight: "semibold", tracking: 0.2pt, fill: color, body,
))

#let note(x, y, width, title, body, color: ink) = at(x, y, block(width: width)[
  #text(size: 11pt, weight: "semibold", fill: color, title) \
  #text(size: 10pt, body)
])

#let node(x, y, width, height, title, body, fill: white) = at(x, y, block(
  width: width,
  height: height,
  fill: fill,
  stroke: (
    rest: 0.55pt + rule,
    left: 1.25pt + if fill == amber { pending } else if fill == green { edge } else { rule },
  ),
  radius: 1.9mm,
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
      stroke: (paint: color, thickness: 0.95pt, dash: dash),
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
