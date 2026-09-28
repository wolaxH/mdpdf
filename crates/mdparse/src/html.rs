//! HTML renderer，輸出格式與 CommonMark 參考實作一致，專門用來跑規格測試。

use crate::ast::{Block, BlockKind, Document, Inline};

pub fn render(doc: &Document) -> String {
    let mut r = Renderer { out: String::new() };
    r.blocks(&doc.blocks, false);
    r.out
}

struct Renderer {
    out: String,
}

impl Renderer {
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
                Inline::SoftBreak => self.out.push('\n'),
                Inline::HardBreak => self.out.push_str("<br />\n"),
            }
        }
    }
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
