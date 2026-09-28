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

#show link: set text(fill: rgb("#1a5fb4"))

// 等寬字型要接 CJK fallback，否則程式碼中的中文註解會變成豆腐字。
// Typst 預設把 raw 縮成 0.8em，中文在程式碼中會顯得太小，這裡稍微放大。
#show raw: set text(font: ("DejaVu Sans Mono", "Noto Sans TC"), size: 1.1em)
// 短的程式碼區塊不跨頁；超過一頁放不下的長區塊只能允許斷開。
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

// tight 清單項目內的區塊（例如嵌套清單）以行距而非段距分隔，與項目間距一致。
#show selector(list.where(tight: true)).or(enum.where(tight: true)): it => {
  show selector(list).or(enum).or(raw.where(block: true)): set block(above: 0.8em, below: 0.8em)
  it
}

// 表格：細格線、表頭加底色與粗體；儲存格內不左右對齊，避免短文字被拉開。
#set table(
  stroke: 0.5pt + luma(190),
  inset: (x: 8pt, y: 5pt),
  fill: (_, y) => if y == 0 { luma(238) },
)
#show table.cell.where(y: 0): set text(weight: "bold")
#show table: set par(justify: false)

// 多段落的腳註：後續段落與第一段的文字對齊，而不是頂到編號左側。
#show footnote.entry: it => {
  let number = numbering(it.note.numbering, ..counter(footnote).at(it.note.location()))
  grid(
    columns: (auto, 1fr),
    column-gutter: 0.2em,
    super(number), it.note.body,
  )
}
