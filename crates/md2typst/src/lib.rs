//! Markdown AST → Typst 原始碼。
//!
//! 所有文字都輸出成 Typst 字串字面值 `#"..."`，區塊則對應到 Typst 的元素函式
//! （`heading`、`list`、`raw` 等），因此 Markdown 內容永遠不會被當成 Typst 標記解讀。

use std::fmt::Write;

use mdparse::{Block, BlockKind, Inline, ListItem};

/// 預設樣式模板，會放在產生的原始碼最前面。
pub const TEMPLATE: &str = include_str!("../../../assets/template.typ");

/// 把 Markdown 轉成完整的 Typst 原始碼（含模板）。
pub fn convert(markdown: &str) -> String {
    let mut out = String::with_capacity(TEMPLATE.len() + markdown.len() * 2);
    out.push_str(TEMPLATE);
    out.push('\n');
    write_body(&mut out, markdown);
    out
}

/// 只產生內文，不含模板。
pub fn body(markdown: &str) -> String {
    let mut out = String::with_capacity(markdown.len() * 2);
    write_body(&mut out, markdown);
    out
}

fn write_body(out: &mut String, markdown: &str) {
    let doc = mdparse::parse(markdown);
    Writer { out }.blocks(&doc.blocks);
}

struct Writer<'o> {
    out: &'o mut String,
}

impl Writer<'_> {
    /// 輸出一串區塊，彼此以空行分隔。每個區塊都以換行結尾。
    fn blocks(&mut self, blocks: &[Block]) {
        let mut first = true;
        for block in blocks {
            // 原始 HTML 不渲染
            if matches!(block.kind, BlockKind::Html(_)) {
                continue;
            }
            if !first {
                self.out.push('\n');
            }
            first = false;
            self.block(block);
        }
    }

    fn block(&mut self, block: &Block) {
        match &block.kind {
            BlockKind::Paragraph(inlines) => {
                self.inlines(inlines);
                self.out.push('\n');
            }
            BlockKind::Heading { level, content } => {
                write!(self.out, "#heading(level: {level})[").unwrap();
                self.inlines(content);
                self.out.push_str("]\n");
            }
            BlockKind::BlockQuote(children) => {
                self.out.push_str("#quote(block: true)[\n");
                self.blocks(children);
                self.out.push_str("]\n");
            }
            BlockKind::List {
                ordered,
                tight,
                items,
            } => {
                match ordered {
                    Some(start) => writeln!(self.out, "#enum(start: {start}, tight: {tight},"),
                    None => writeln!(self.out, "#list(tight: {tight},"),
                }
                .unwrap();
                for item in items {
                    self.list_item(item);
                }
                self.out.push_str(")\n");
            }
            BlockKind::CodeBlock { lang, code } => {
                self.out.push_str("#raw(block: true, ");
                if let Some(lang) = lang {
                    self.out.push_str("lang: ");
                    push_str_literal(self.out, lang);
                    self.out.push_str(", ");
                }
                push_str_literal(self.out, code.strip_suffix('\n').unwrap_or(code));
                self.out.push_str(")\n");
            }
            BlockKind::Html(_) => {}
            BlockKind::ThematicBreak => self.out.push_str("#line(length: 100%)\n"),
        }
    }

    fn list_item(&mut self, item: &ListItem) {
        self.out.push('[');
        if !item.blocks.is_empty() {
            self.out.push('\n');
            self.blocks(&item.blocks);
        }
        self.out.push_str("],\n");
    }

    fn inlines(&mut self, inlines: &[Inline]) {
        // 相鄰的文字合併成一個字串字面值
        let mut run = String::new();
        for (i, inline) in inlines.iter().enumerate() {
            match inline {
                Inline::Text(text) => run.push_str(text),
                Inline::SoftBreak => {
                    let prev = match inlines.get(i.wrapping_sub(1)) {
                        Some(Inline::Text(_)) => run.chars().next_back(),
                        _ => None,
                    };
                    let next = match inlines.get(i + 1) {
                        Some(Inline::Text(text)) => text.chars().next(),
                        _ => None,
                    };
                    if soft_break(prev, next) {
                        run.push(' ');
                    }
                }
                Inline::HardBreak => {
                    self.flush_text(&mut run);
                    self.out.push_str("#linebreak()");
                }
                Inline::Code(code) => {
                    self.flush_text(&mut run);
                    self.out.push_str("#raw(");
                    push_str_literal(self.out, code);
                    self.out.push(')');
                }
            }
        }
        self.flush_text(&mut run);
    }

    fn flush_text(&mut self, run: &mut String) {
        if !run.is_empty() {
            push_text(self.out, run);
            run.clear();
        }
    }
}

/// 原始碼中的軟換行在排版時是否應成為空白。
///
/// 兩側任一邊是 CJK 字元時不插入空白，否則中文段落每個換行處都會多出一個空格。
/// 鄰接的不是文字（例如行內程式碼）時視為非 CJK。
pub fn soft_break(prev: Option<char>, next: Option<char>) -> bool {
    !(prev.is_some_and(is_cjk) || next.is_some_and(is_cjk))
}

/// 粗略判斷是否為 CJK 文字或全形標點。
pub fn is_cjk(c: char) -> bool {
    matches!(c,
        '\u{2E80}'..='\u{9FFF}'      // 部首、標點、假名、注音、CJK 統一表意文字
        | '\u{AC00}'..='\u{D7AF}'    // 韓文音節
        | '\u{F900}'..='\u{FAFF}'    // 相容表意文字
        | '\u{FE30}'..='\u{FE4F}'    // 相容形式（直排標點）
        | '\u{FF00}'..='\u{FFEF}'    // 全形字元
        | '\u{20000}'..='\u{3FFFF}'  // 擴充 B 之後
    )
}

/// 以 Typst 字串字面值 `#"..."` 輸出純文字。
///
/// 字串內只有 `"` 與 `\` 有特殊意義，因此文字中的 `*`、`_`、`#`、`$`、`@` 等
/// 標記字元不會被 Typst 解讀，從根本避免注入問題。
pub fn push_text(out: &mut String, text: &str) {
    out.push('#');
    push_str_literal(out, text);
}

/// 輸出 Typst 字串字面值（含前後引號）。
pub fn push_str_literal(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => write!(out, "\\u{{{:x}}}", c as u32).unwrap(),
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lit(text: &str) -> String {
        let mut out = String::new();
        push_str_literal(&mut out, text);
        out
    }

    #[test]
    fn str_literal_escapes_only_quote_backslash_and_controls() {
        assert_eq!(lit("a*b_#$@<[]>`"), r#""a*b_#$@<[]>`""#);
        assert_eq!(lit(r#"say "hi" \o/"#), r#""say \"hi\" \\o/""#);
        assert_eq!(lit("a\tb\nc\u{7}"), r#""a\tb\nc\u{7}""#);
    }

    #[test]
    fn soft_breaks_join_cjk_without_space() {
        assert_eq!(
            body("中文第一行\n第二行\nmixed\nEnglish 行"),
            "#\"中文第一行第二行mixed English 行\"\n"
        );
        assert_eq!(body("`code`\nnext"), "#raw(\"code\")#\" next\"\n");
    }

    #[test]
    fn convert_prepends_template() {
        let out = convert("你好");
        assert!(out.starts_with(TEMPLATE));
        assert!(out.ends_with("\n#\"你好\"\n"));
    }
}
