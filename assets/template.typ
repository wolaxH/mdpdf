// mdpdf default style template. Codegen appends the converted body after this template.

// The body uses a sans-serif (hei) face; Noto Sans TC has its own Latin glyphs, so mixed CJK/Latin text stays consistent.
#set text(font: "Noto Sans TC", size: 11pt, lang: "zh", region: "tw")
#set page(paper: "a4", margin: 2.5cm, numbering: "1")

#set par(justify: true, leading: 0.8em, spacing: 1.2em)
// Typst's default second-level marker ‣ is missing from Noto Sans TC; use markers the font has.
#set list(indent: 0.4em, marker: ([•], [◦], [▪]))
#set enum(indent: 0.4em)
#set line(stroke: 0.5pt + luma(170))

#show heading: set block(above: 1.4em, below: 0.8em)

#show link: set text(fill: rgb("#1a5fb4"))

// Monospace fonts need a CJK fallback, otherwise CJK comments in code render as tofu.
// Typst shrinks raw text to 0.8em, which makes CJK in code too small; enlarge it slightly.
#show raw: set text(font: ("DejaVu Sans Mono", "Noto Sans TC"), size: 1.1em)
// Short code blocks do not break across pages; blocks too long for one page must be allowed to break.
#show raw.where(block: true): it => block(
  width: 100%,
  fill: luma(246),
  inset: 10pt,
  radius: 4pt,
  breakable: it.text.split("\n").len() > 30,
  it,
)
#show raw.where(block: false): box.with(
  fill: luma(240),
  inset: (x: 3pt),
  outset: (y: 3pt),
  radius: 2pt,
)

#show quote.where(block: true): it => block(
  stroke: (left: 2pt + luma(200)),
  inset: (left: 12pt, y: 4pt),
  text(fill: luma(80), it.body),
)

// Blocks inside tight list items (e.g. nested lists) are separated by leading, not paragraph spacing, matching the item gap.
#show selector(list.where(tight: true)).or(enum.where(tight: true)): it => {
  show selector(list).or(enum).or(raw.where(block: true)): set block(above: 0.8em, below: 0.8em)
  it
}

// Tables: thin rules, shaded bold header; no justification in cells so short text is not stretched.
#set table(
  stroke: 0.5pt + luma(190),
  inset: (x: 8pt, y: 5pt),
  fill: (_, y) => if y == 0 { luma(238) },
)
#show table.cell.where(y: 0): set text(weight: "bold")
#show table: set par(justify: false)

// Multi-paragraph footnotes: later paragraphs align with the first paragraph's text instead of the number.
#show footnote.entry: it => {
  let number = numbering(it.note.numbering, ..counter(footnote).at(it.note.location()))
  grid(
    columns: (auto, 1fr),
    column-gutter: 0.2em,
    super(number), it.note.body,
  )
}

// Noto Sans TC has no italic and Typst does not synthesize one, so emphasis is slanted with a
// skew. Each CJK character and each Latin word is skewed on its own, so lines can still break.
// Real italics are turned off first, keeping the slant consistent for fonts that do have them.
#show emph: it => {
  set text(style: "normal")
  show regex("\p{Han}|\p{Hiragana}|\p{Katakana}|\p{Hangul}|[^\p{Han}\p{Hiragana}\p{Katakana}\p{Hangul}\s]+"): w => box(skew(ax: -12deg, reflow: false, w))
  it.body
}
