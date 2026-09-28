//! HTML renderer，輸出格式與 CommonMark／GFM 參考實作一致，專門用來跑規格測試。

use std::collections::HashMap;

use crate::ast::{Align, Block, BlockKind, Cell, Document, Inline};
use crate::scan::normalize_label;

pub fn render(doc: &Document) -> String {
    let mut r = Renderer {
        out: String::new(),
        footnote_defs: HashMap::new(),
        footnote_order: Vec::new(),
    };
    collect_footnotes(&doc.blocks, &mut r.footnote_defs);
    r.blocks(&doc.blocks, false);
    r.footnotes();
    r.out
}

fn collect_footnotes<'d, 'a>(blocks: &'d [Block<'a>], defs: &mut HashMap<String, &'d [Block<'a>]>) {
    for block in blocks {
        match &block.kind {
            BlockKind::FootnoteDef { label, blocks } => {
                defs.entry(normalize_label(label)).or_insert(blocks);
                collect_footnotes(blocks, defs);
            }
            BlockKind::BlockQuote(children) => collect_footnotes(children, defs),
            BlockKind::List { items, .. } => {
                for item in items {
                    collect_footnotes(&item.blocks, defs);
                }
            }
            _ => {}
        }
    }
}

struct Renderer<'d, 'a> {
    out: String,
    footnote_defs: HashMap<String, &'d [Block<'a>]>,
    /// 依第一次被引用的順序排列的腳註標籤。
    footnote_order: Vec<String>,
}

impl<'d, 'a> Renderer<'d, 'a> {
    /// 換行，但不重複輸出。
    fn cr(&mut self) {
        if !self.out.is_empty() && !self.out.ends_with('\n') {
            self.out.push('\n');
        }
    }

    fn blocks(&mut self, blocks: &[Block], tight: bool) {
        for block in blocks {
            self.block(block, tight);
        }
    }

    fn block(&mut self, block: &Block, tight: bool) {
        match &block.kind {
            BlockKind::Paragraph(inlines) if tight => self.inlines(inlines),
            BlockKind::Paragraph(inlines) => {
                self.cr();
                self.out.push_str("<p>");
                self.inlines(inlines);
                self.out.push_str("</p>");
                self.cr();
            }
            BlockKind::Heading { level, content } => {
                self.cr();
                self.out.push_str(&format!("<h{level}>"));
                self.inlines(content);
                self.out.push_str(&format!("</h{level}>"));
                self.cr();
            }
            BlockKind::BlockQuote(children) => {
                self.cr();
                self.out.push_str("<blockquote>");
                self.cr();
                self.blocks(children, false);
                self.cr();
                self.out.push_str("</blockquote>");
                self.cr();
            }
            BlockKind::List {
                ordered,
                tight,
                items,
            } => {
                let tag = if ordered.is_some() { "ol" } else { "ul" };
                self.cr();
                match ordered {
                    Some(start) if *start != 1 => {
                        self.out.push_str(&format!("<ol start=\"{start}\">"))
                    }
                    _ => self.out.push_str(&format!("<{tag}>")),
                }
                self.cr();
                for item in items {
                    self.out.push_str("<li>");
                    match item.task {
                        Some(true) => self
                            .out
                            .push_str("<input checked=\"\" disabled=\"\" type=\"checkbox\"> "),
                        Some(false) => self
                            .out
                            .push_str("<input disabled=\"\" type=\"checkbox\"> "),
                        None => {}
                    }
                    self.blocks(&item.blocks, *tight);
                    self.out.push_str("</li>");
                    self.cr();
                }
                self.cr();
                self.out.push_str(&format!("</{tag}>"));
                self.cr();
            }
            BlockKind::CodeBlock { lang, code } => {
                self.cr();
                self.out.push_str("<pre><code");
                if let Some(lang) = lang {
                    self.out.push_str(" class=\"language-");
                    escape(&mut self.out, lang);
                    self.out.push('"');
                }
                self.out.push('>');
                escape(&mut self.out, code);
                self.out.push_str("</code></pre>");
                self.cr();
            }
            BlockKind::Html(html) => {
                self.cr();
                self.out.push_str(html);
                self.cr();
            }
            BlockKind::Table { align, head, rows } => {
                self.cr();
                self.out.push_str("<table>\n<thead>\n");
                self.table_row(head, align, "th");
                self.out.push_str("</thead>\n");
                if !rows.is_empty() {
                    self.out.push_str("<tbody>\n");
                    for row in rows {
                        self.table_row(row, align, "td");
                    }
                    self.out.push_str("</tbody>\n");
                }
                self.out.push_str("</table>\n");
            }
            // 腳註內容在文件末尾輸出
            BlockKind::FootnoteDef { .. } => {}
            BlockKind::ThematicBreak => {
                self.cr();
                self.out.push_str("<hr />");
                self.cr();
            }
        }
    }

    fn inlines(&mut self, inlines: &[Inline]) {
        for inline in inlines {
            match inline {
                Inline::Text(text) => escape(&mut self.out, text),
                Inline::Code(code) => {
                    self.out.push_str("<code>");
                    escape(&mut self.out, code);
                    self.out.push_str("</code>");
                }
                Inline::Emph(children) => {
                    self.out.push_str("<em>");
                    self.inlines(children);
                    self.out.push_str("</em>");
                }
                Inline::Strong(children) => {
                    self.out.push_str("<strong>");
                    self.inlines(children);
                    self.out.push_str("</strong>");
                }
                Inline::Link {
                    url,
                    title,
                    content,
                } => {
                    self.out.push_str("<a href=\"");
                    escape(&mut self.out, &normalize_uri(url));
                    self.out.push('"');
                    self.title(title.as_deref());
                    self.out.push('>');
                    self.inlines(content);
                    self.out.push_str("</a>");
                }
                Inline::Image { url, title, alt } => {
                    self.out.push_str("<img src=\"");
                    escape(&mut self.out, &normalize_uri(url));
                    self.out.push_str("\" alt=\"");
                    let mut text = String::new();
                    alt_text(alt, &mut text);
                    escape(&mut self.out, &text);
                    self.out.push('"');
                    self.title(title.as_deref());
                    self.out.push_str(" />");
                }
                Inline::Html(html) => self.out.push_str(html),
                Inline::Strike(children) => {
                    self.out.push_str("<del>");
                    self.inlines(children);
                    self.out.push_str("</del>");
                }
                Inline::FootnoteRef(label) => {
                    let key = normalize_label(label);
                    let n = match self.footnote_order.iter().position(|l| *l == key) {
                        Some(i) => i + 1,
                        None => {
                            self.footnote_order.push(key.clone());
                            self.footnote_order.len()
                        }
                    };
                    let id = escaped(&key.to_lowercase());
                    self.out.push_str(&format!(
                        "<sup class=\"footnote-ref\"><a href=\"#fn-{id}\" id=\"fnref-{id}\">{n}</a></sup>"
                    ));
                }
                Inline::SoftBreak => self.out.push('\n'),
                Inline::HardBreak => self.out.push_str("<br />\n"),
            }
        }
    }

    fn table_row(&mut self, cells: &[Cell], align: &[Align], tag: &str) {
        self.out.push_str("<tr>\n");
        for (cell, align) in cells.iter().zip(align) {
            let attr = match align {
                Align::None => "",
                Align::Left => " align=\"left\"",
                Align::Center => " align=\"center\"",
                Align::Right => " align=\"right\"",
            };
            self.out.push_str(&format!("<{tag}{attr}>"));
            self.inlines(cell);
            self.out.push_str(&format!("</{tag}>\n"));
        }
        self.out.push_str("</tr>\n");
    }

    /// 被引用過的腳註，依引用順序輸出。腳註內再引用新腳註時會接在後面。
    fn footnotes(&mut self) {
        if self.footnote_order.is_empty() {
            return;
        }
        self.cr();
        self.out.push_str("<section class=\"footnotes\">\n<ol>\n");
        let mut i = 0;
        while let Some(key) = self.footnote_order.get(i).cloned() {
            let id = escaped(&key.to_lowercase());
            self.out.push_str(&format!("<li id=\"fn-{id}\">\n"));
            if let Some(blocks) = self.footnote_defs.get(&key).copied() {
                self.blocks(blocks, false);
            }
            self.cr();
            self.out.push_str("</li>\n");
            i += 1;
        }
        self.out.push_str("</ol>\n</section>\n");
    }

    fn title(&mut self, title: Option<&str>) {
        if let Some(title) = title.filter(|t| !t.is_empty()) {
            self.out.push_str(" title=\"");
            escape(&mut self.out, title);
            self.out.push('"');
        }
    }
}

/// 圖片 alt：只保留文字，與參考實作相同。
fn alt_text(inlines: &[Inline], out: &mut String) {
    for inline in inlines {
        match inline {
            Inline::Text(s) | Inline::Code(s) | Inline::Html(s) => out.push_str(s),
            Inline::Emph(v) | Inline::Strong(v) | Inline::Strike(v) => alt_text(v, out),
            Inline::FootnoteRef(_) => {}
            Inline::Link { content, .. } => alt_text(content, out),
            Inline::Image { alt, .. } => alt_text(alt, out),
            Inline::SoftBreak | Inline::HardBreak => out.push('\n'),
        }
    }
}

/// 百分比編碼 URL 中不安全的字元，保留既有的 `%XX` 序列。
fn normalize_uri(url: &str) -> String {
    const SAFE: &[u8] = b";/?:@&=+$,-_.!~*'()#";
    let b = url.as_bytes();
    let mut out = String::with_capacity(url.len());
    for (i, &c) in b.iter().enumerate() {
        let escaped_seq = c == b'%'
            && b.len() > i + 2
            && b[i + 1].is_ascii_hexdigit()
            && b[i + 2].is_ascii_hexdigit();
        if c.is_ascii_alphanumeric() || SAFE.contains(&c) || escaped_seq {
            out.push(c as char);
        } else {
            out.push_str(&format!("%{c:02X}"));
        }
    }
    out
}

fn escaped(text: &str) -> String {
    let mut out = String::new();
    escape(&mut out, text);
    out
}

fn escape(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
}
