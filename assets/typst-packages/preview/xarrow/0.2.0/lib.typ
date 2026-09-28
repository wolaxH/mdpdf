// 可延展的箭頭，上方放文字：`xarrow(sym: sym.arrow.r.long, [text])`。
// 以 Typst 內建的 stretch 實作：attachment 的 base 使用 stretch 時，
// 會自動延展到上下標的寬度。
#let xarrow(sym: sym.arrow.long, margin: 0.15em, width: auto, ..rest, body) = {
  let size = if width == auto { 100% + 2 * margin } else { width }
  $stretch(#sym, size: #size)^#body$
}
