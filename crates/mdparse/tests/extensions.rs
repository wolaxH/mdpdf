//! mdpdf 自有擴充的解析測試（規格測試涵蓋不到的部分）。

use mdparse::{BlockKind, Options, parse, parse_with};

fn kinds(markdown: &str, options: Options) -> Vec<String> {
    parse_with(markdown, options)
        .blocks
        .iter()
        .map(|b| match &b.kind {
            BlockKind::Paragraph(_) => "p".into(),
            BlockKind::Heading { .. } => "h".into(),
            BlockKind::PageBreak => "pagebreak".into(),
            BlockKind::Html(_) => "html".into(),
            BlockKind::MathBlock(_) => "math".into(),
            BlockKind::List { .. } => "list".into(),
            other => format!("{other:?}"),
        })
        .collect()
}

#[test]
fn page_break_marker() {
    let md = "# 一\n\n<!-- pagebreak -->\n\n# 二\n段落\n<!--PageBreak-->\n接續\n";
    assert_eq!(
        kinds(md, Options::default()),
        ["h", "pagebreak", "h", "p", "pagebreak", "p"]
    );
    // 純 CommonMark 下只是 HTML 註解
    assert_eq!(
        kinds(md, Options::commonmark()),
        ["h", "html", "h", "p", "html", "p"]
    );
    // 其他註解、同一行還有其他內容時都不是換頁
    assert_eq!(
        kinds(
            "<!-- note -->\n\n<!-- pagebreak --> x\n",
            Options::default()
        ),
        ["html", "html"]
    );
}

#[test]
fn math_blocks() {
    assert_eq!(
        kinds(
            "$$\nx\n$$\n\n$$ y $$\n\n```math\nz\n```\n",
            Options::default()
        ),
        ["math", "math", "math"]
    );
    let doc = parse("$$\na + b\n= c $$\n");
    assert!(matches!(&doc.blocks[0].kind, BlockKind::MathBlock(tex) if tex == "a + b\n= c"));
    // 同一行關閉後還有文字：是段落中的行內公式
    assert_eq!(kinds("$$ x $$ 之後的文字\n", Options::default()), ["p"]);
}
