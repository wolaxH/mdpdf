// Stretchable arrow with text above it: `xarrow(sym: sym.arrow.r.long, [text])`.
// Implemented with Typst's built-in stretch: when the base of an attachment uses stretch,
// it grows to the width of its attachments.
#let xarrow(sym: sym.arrow.long, margin: 0.15em, width: auto, ..rest, body) = {
  let size = if width == auto { 100% + 2 * margin } else { width }
  $stretch(#sym, size: #size)^#body$
}
