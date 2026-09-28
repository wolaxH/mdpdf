// mdpdf 預設樣式模板。codegen 會把轉換後的內文接在這份模板之後。

// 預設內文用黑體；Noto Sans TC 本身也有拉丁字形，中英混排時筆畫風格一致。
#set text(font: "Noto Sans TC", size: 11pt, lang: "zh", region: "tw")
#set page(paper: "a4", margin: 2.5cm, numbering: "1")

#set par(justify: true, leading: 0.8em, spacing: 1.2em)
// Typst 預設的第二層符號 ‣ 不在 Noto Sans TC 裡，改用字型內有的符號。
#set list(indent: 0.4em, marker: ([•], [◦], [▪]))
#set enum(indent: 0.4em)
#set line(stroke: 0.5pt + luma(170))

#show heading: set block(above: 1.4em, below: 0.8em)

// 等寬字型要接 CJK fallback，否則程式碼中的中文註解會變成豆腐字。
#show raw: set text(font: ("DejaVu Sans Mono", "Noto Sans TC"))
#show raw.where(block: true): block.with(
  width: 100%,
  fill: luma(246),
  inset: 10pt,
  radius: 4pt,
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

// tight 清單項目內的區塊（例如嵌套清單）以行距而非段距分隔，與項目間距一致。
#show selector(list.where(tight: true)).or(enum.where(tight: true)): it => {
  show selector(list).or(enum).or(raw.where(block: true)): set block(above: 0.8em, below: 0.8em)
  it
}
