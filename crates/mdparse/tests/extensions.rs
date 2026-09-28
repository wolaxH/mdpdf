//! Parser tests for mdpdf's own extensions (not covered by the spec tests).

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
    // In plain CommonMark it is just an HTML comment
    assert_eq!(
        kinds(md, Options::commonmark()),
        ["h", "html", "h", "p", "html", "p"]
    );
    // Other comments, or extra content on the same line, are not page breaks
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
    // Text after the closing `$$` on the same line: inline math in a paragraph
    assert_eq!(kinds("$$ x $$ 之後的文字\n", Options::default()), ["p"]);
}
