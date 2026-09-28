//! Markdown AST → Typst 原始碼。
//!
//! 所有文字都輸出成 Typst 字串字面值 `#"..."`，區塊與格式則對應到 Typst 的元素函式
//! （`heading`、`list`、`raw`、`strong` 等），因此 Markdown 內容永遠不會被當成
//! Typst 標記解讀。

use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::path::{Path, PathBuf};

use mdparse::{Align, Block, BlockKind, Cell, Inline, ListItem, is_cjk, normalize_label};

/// codegen 產生的原始碼會呼叫的輔助函式。與樣式無關，永遠放在最前面。
pub const PRELUDE: &str = r#"// mdpdf 輔助函式
#let mdpdf-checkbox(checked) = box(
  width: 0.8em,
  height: 0.8em,
  baseline: 0.1em,
  stroke: 0.6pt + luma(80),
  radius: 1.5pt,
  if checked { place(center + horizon, text(size: 0.75em, weight: "bold", "✓")) },
)
"#;

/// 預設樣式模板，接在 [`PRELUDE`] 之後。
pub const TEMPLATE: &str = include_str!("../../../assets/template.typ");

/// Typst 能直接讀取的圖片格式（依副檔名判斷）。
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "svg", "pdf"];

#[derive(Debug, Clone, Default)]
pub struct Options {
    /// 圖片相對路徑的基準目錄。設定時會檢查圖片是否存在；未設定則不檢查。
    pub base_dir: Option<PathBuf>,
    /// Markdown 解析選項。
    pub parse: mdparse::Options,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    /// Markdown 原始行號（目前是所在區塊的起始行）。
    pub line: u32,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct Output {
    pub source: String,
    pub warnings: Vec<Warning>,
}

/// 把 Markdown 轉成完整的 Typst 原始碼（含模板）。
pub fn convert(markdown: &str, options: &Options) -> Output {
    let mut out = Output {
        source: String::with_capacity(PRELUDE.len() + TEMPLATE.len() + markdown.len() * 2),
        warnings: Vec::new(),
    };
    out.source.push_str(PRELUDE);
    out.source.push('\n');
    out.source.push_str(TEMPLATE);
    out.source.push('\n');
    write_body(&mut out, markdown, options);
    out
}

/// 只產生內文，不含模板。
pub fn body(markdown: &str, options: &Options) -> Output {
    let mut out = Output {
        source: String::with_capacity(markdown.len() * 2),
        warnings: Vec::new(),
    };
    write_body(&mut out, markdown, options);
    out
}

fn write_body(out: &mut Output, markdown: &str, options: &Options) {
    let doc = mdparse::parse_with(markdown, options.parse);
    let mut footnote_defs = HashMap::new();
    collect_footnotes(&doc.blocks, &mut footnote_defs);
    Writer {
        out: &mut out.source,
        warnings: &mut out.warnings,
        options,
        line: 1,
        footnote_defs,
        footnote_ids: HashMap::new(),
        footnotes_in_progress: HashSet::new(),
    }
    .blocks(&doc.blocks);
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

struct Writer<'o, 'd, 'a> {
    out: &'o mut String,
    warnings: &'o mut Vec<Warning>,
    options: &'o Options,
    /// 目前處理中區塊的起始行，用於警告。
    line: u32,
    /// 腳註定義（正規化標籤 → 內容）。
    footnote_defs: HashMap<String, &'d [Block<'a>]>,
    /// 已輸出的腳註及其 Typst 標籤編號，重複引用時指向同一個腳註。
    footnote_ids: HashMap<String, usize>,
    /// 正在輸出的腳註，防止腳註引用自己造成無窮遞迴。
    footnotes_in_progress: HashSet<String>,
}

impl Writer<'_, '_, '_> {
    fn warn(&mut self, message: String) {
        self.warnings.push(Warning {
            line: self.line,
            message,
        });
    }

    /// 輸出一串區塊，彼此以空行分隔。每個區塊都以換行結尾。
    fn blocks(&mut self, blocks: &[Block]) {
        let mut first = true;
        for block in blocks {
            // 原始 HTML 不渲染；腳註定義在引用處輸出
            if matches!(
                block.kind,
                BlockKind::Html(_) | BlockKind::FootnoteDef { .. }
            ) {
                continue;
            }
            if !first {
                self.out.push('\n');
            }
            first = false;
            self.line = block.span.line;
            self.block(block);
        }
    }

    fn block(&mut self, block: &Block) {
        match &block.kind {
            BlockKind::Paragraph(inlines) if is_image_paragraph(inlines) => {
                // 只有圖片的段落：圖片獨立成區塊並置中
                for inline in inlines {
                    if let Inline::Image { url, alt, .. } = inline {
                        self.image(url, alt, true);
                        self.out.push('\n');
                    }
                }
            }
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
                    Some(start) => write!(self.out, "#enum(start: {start}, tight: {tight},"),
                    None => write!(self.out, "#list(tight: {tight},"),
                }
                .unwrap();
                // 任務清單以核取方塊取代項目符號
                if ordered.is_none() && items.iter().any(|i| i.task.is_some()) {
                    self.out.push_str(" marker: [],");
                }
                self.out.push('\n');
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
            BlockKind::Html(_) | BlockKind::FootnoteDef { .. } => {}
            BlockKind::ThematicBreak => self.out.push_str("#line(length: 100%)\n"),
            BlockKind::Table { align, head, rows } => self.table(align, head, rows),
        }
    }

    fn table(&mut self, align: &[Align], head: &[Cell], rows: &[Vec<Cell>]) {
        writeln!(self.out, "#table(\n  columns: {},", align.len()).unwrap();
        let align: Vec<_> = align
            .iter()
            .map(|a| match a {
                // 明確指定靠左，外層的置中才不會影響儲存格
                Align::None | Align::Left => "left",
                Align::Center => "center",
                Align::Right => "right",
            })
            .collect();
        // 單欄時要加逗號，否則 `(left)` 不是陣列
        let trailing = if align.len() == 1 { "," } else { "" };
        writeln!(self.out, "  align: ({}{trailing}),", align.join(", ")).unwrap();
        self.out.push_str("  table.header(");
        self.table_cells(head);
        self.out.push_str("),\n");
        for row in rows {
            self.out.push_str("  ");
            self.table_cells(row);
            self.out.push('\n');
        }
        self.out.push_str(")\n");
    }

    fn table_cells(&mut self, cells: &[Cell]) {
        for cell in cells {
            self.out.push('[');
            self.inlines(cell);
            self.out.push_str("], ");
        }
    }

    fn list_item(&mut self, item: &ListItem) {
        self.out.push('[');
        if let Some(checked) = item.task {
            write!(self.out, "#mdpdf-checkbox({checked})").unwrap();
        }
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
                    let prev = i.checked_sub(1).and_then(|p| last_char(&inlines[p]));
                    let next = inlines.get(i + 1).and_then(first_char);
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
                Inline::Emph(children) => {
                    self.flush_text(&mut run);
                    self.out.push_str("#emph[");
                    self.inlines(children);
                    self.out.push(']');
                }
                Inline::Strong(children) => {
                    self.flush_text(&mut run);
                    self.out.push_str("#strong[");
                    self.inlines(children);
                    self.out.push(']');
                }
                Inline::Strike(children) => {
                    self.flush_text(&mut run);
                    self.out.push_str("#strike[");
                    self.inlines(children);
                    self.out.push(']');
                }
                Inline::FootnoteRef(label) => {
                    self.flush_text(&mut run);
                    self.footnote(label);
                }
                Inline::Link { url, content, .. } => {
                    self.flush_text(&mut run);
                    if url.is_empty() {
                        // Typst 不接受空的連結目標，只輸出文字
                        self.inlines(content);
                    } else {
                        self.out.push_str("#link(");
                        push_str_literal(self.out, url);
                        self.out.push_str(")[");
                        self.inlines(content);
                        self.out.push(']');
                    }
                }
                Inline::Image { url, alt, .. } => {
                    self.flush_text(&mut run);
                    self.image(url, alt, false);
                }
                Inline::Html(html) => match html_tag_name(html) {
                    Some(name) if name.eq_ignore_ascii_case("br") => {
                        self.flush_text(&mut run);
                        self.out.push_str("#linebreak()");
                    }
                    // 標準 HTML 標籤不渲染（標籤之間的文字仍會保留）
                    Some(name) if is_html_element(name) => {}
                    // 不是 HTML 元素的「標籤」多半是 `Vec<T>` 這類文字，原樣輸出
                    Some(_) => run.push_str(html),
                    // 註解、處理指令等
                    None => {}
                },
            }
        }
        self.flush_text(&mut run);
    }

    /// 第一次引用時輸出完整腳註並加上標籤，之後的引用指向同一個腳註。
    fn footnote(&mut self, label: &str) {
        let key = normalize_label(label);
        if let Some(id) = self.footnote_ids.get(&key) {
            write!(self.out, "#footnote(<mdpdf-fn-{id}>)").unwrap();
            return;
        }
        let Some(blocks) = self.footnote_defs.get(&key).copied() else {
            return;
        };
        if !self.footnotes_in_progress.insert(key.clone()) {
            return;
        }
        let id = self.footnote_ids.len() + 1;
        self.footnote_ids.insert(key.clone(), id);
        let line = self.line;
        self.out.push_str("#footnote[");
        self.blocks(blocks);
        write!(self.out, "]<mdpdf-fn-{id}>").unwrap();
        self.line = line;
        self.footnotes_in_progress.remove(&key);
    }

    fn flush_text(&mut self, run: &mut String) {
        if !run.is_empty() {
            push_text(self.out, run);
            run.clear();
        }
    }

    fn image(&mut self, url: &str, alt: &[Inline], block: bool) {
        let alt = mdparse::plain_text(alt);
        let path = match self.check_image(url) {
            Ok(path) => path,
            Err(message) => {
                self.warn(message);
                // 佔位框：顯示 alt 文字（沒有就顯示路徑）
                let label = if alt.is_empty() { url } else { &alt };
                self.out
                    .push_str(if block { "#align(center, " } else { "#" });
                self.out
                    .push_str("box(stroke: 0.5pt + luma(150), inset: 4pt, text(fill: luma(100), ");
                push_str_literal(self.out, &format!("[圖片：{label}]"));
                self.out.push_str("))");
                if block {
                    self.out.push(')');
                }
                return;
            }
        };
        self.out.push_str(if block {
            "#align(center, image("
        } else {
            "#box(image("
        });
        push_str_literal(self.out, &path);
        if !alt.is_empty() {
            self.out.push_str(", alt: ");
            push_str_literal(self.out, &alt);
        }
        self.out.push_str("))");
    }

    /// 檢查圖片是否可用，回傳要交給 Typst 的路徑。
    fn check_image(&self, url: &str) -> Result<String, String> {
        if url.is_empty() {
            return Err("圖片路徑是空的".into());
        }
        if url.contains("://") || url.starts_with("data:") {
            return Err(format!("尚不支援遠端圖片：{url}"));
        }
        let path = percent_decode(url.strip_prefix("file:").unwrap_or(url));
        let ext = Path::new(&path)
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if !IMAGE_EXTENSIONS.contains(&ext.as_str()) {
            return Err(format!(
                "不支援的圖片格式：{url}（支援 {}）",
                IMAGE_EXTENSIONS.join("、")
            ));
        }
        if let Some(base) = &self.options.base_dir
            && !base.join(&path).is_file()
        {
            return Err(format!("找不到圖片：{url}"));
        }
        Ok(path)
    }
}

/// 標準 HTML 元素名稱（已排序）。
const HTML_ELEMENTS: &[&str] = &[
    "a",
    "abbr",
    "address",
    "area",
    "article",
    "aside",
    "audio",
    "b",
    "bdi",
    "bdo",
    "blockquote",
    "body",
    "br",
    "button",
    "canvas",
    "caption",
    "center",
    "cite",
    "code",
    "col",
    "colgroup",
    "data",
    "dd",
    "del",
    "details",
    "dfn",
    "dialog",
    "div",
    "dl",
    "dt",
    "em",
    "embed",
    "fieldset",
    "figcaption",
    "figure",
    "font",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hr",
    "html",
    "i",
    "iframe",
    "img",
    "input",
    "ins",
    "kbd",
    "label",
    "legend",
    "li",
    "link",
    "main",
    "mark",
    "meta",
    "meter",
    "nav",
    "noscript",
    "object",
    "ol",
    "optgroup",
    "option",
    "output",
    "p",
    "param",
    "picture",
    "pre",
    "progress",
    "q",
    "rp",
    "rt",
    "ruby",
    "s",
    "samp",
    "script",
    "section",
    "select",
    "small",
    "source",
    "span",
    "strike",
    "strong",
    "style",
    "sub",
    "summary",
    "sup",
    "table",
    "tbody",
    "td",
    "template",
    "textarea",
    "tfoot",
    "th",
    "thead",
    "time",
    "title",
    "tr",
    "track",
    "tt",
    "u",
    "ul",
    "var",
    "video",
    "wbr",
];

/// 開始或結束標籤的名稱；註解等其他原始 HTML 回傳 `None`。
fn html_tag_name(html: &str) -> Option<&str> {
    let rest = html.strip_prefix("</").or_else(|| html.strip_prefix('<'))?;
    let len = rest
        .bytes()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == b'-')
        .count();
    (len > 0).then(|| &rest[..len])
}

fn is_html_element(name: &str) -> bool {
    let lower = name.bytes().map(|c| c.to_ascii_lowercase());
    HTML_ELEMENTS
        .binary_search_by(|e| e.bytes().cmp(lower.clone()))
        .is_ok()
}

/// 段落是否只由圖片組成（圖片之間只有換行或空白）。
fn is_image_paragraph(inlines: &[Inline]) -> bool {
    inlines.iter().any(|i| matches!(i, Inline::Image { .. }))
        && inlines.iter().all(|i| match i {
            Inline::Image { .. } | Inline::SoftBreak | Inline::HardBreak => true,
            Inline::Text(t) => t.trim().is_empty(),
            _ => false,
        })
}

fn first_char(inline: &Inline) -> Option<char> {
    match inline {
        Inline::Text(t) => t.chars().next(),
        Inline::Emph(v)
        | Inline::Strong(v)
        | Inline::Strike(v)
        | Inline::Link { content: v, .. } => v.first().and_then(first_char),
        // 行內程式碼、圖片等視為非 CJK
        _ => Some('a'),
    }
}

fn last_char(inline: &Inline) -> Option<char> {
    match inline {
        Inline::Text(t) => t.chars().next_back(),
        Inline::Emph(v)
        | Inline::Strong(v)
        | Inline::Strike(v)
        | Inline::Link { content: v, .. } => v.last().and_then(last_char),
        _ => Some('a'),
    }
}

/// 解碼 URL 中的 `%XX`，用於本機檔案路徑。
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(hex) = s.get(i + 1..i + 3)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 原始碼中的軟換行在排版時是否應成為空白。
///
/// 兩側任一邊是 CJK 字元時不插入空白，否則中文段落每個換行處都會多出一個空格。
/// 鄰接的不是文字（例如行內程式碼）時視為非 CJK。
pub fn soft_break(prev: Option<char>, next: Option<char>) -> bool {
    !(prev.is_some_and(is_cjk) || next.is_some_and(is_cjk))
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

    fn typst(markdown: &str) -> String {
        body(markdown, &Options::default()).source
    }

    #[test]
    fn soft_breaks_join_cjk_without_space() {
        assert_eq!(
            typst("中文第一行\n第二行\nmixed\nEnglish 行"),
            "#\"中文第一行第二行mixed English 行\"\n"
        );
        assert_eq!(typst("`code`\nnext"), "#raw(\"code\")#\" next\"\n");
        assert_eq!(typst("**粗體**\n接續"), "#strong[#\"粗體\"]#\"接續\"\n");
    }

    #[test]
    fn convert_prepends_template() {
        let out = convert("你好", &Options::default()).source;
        assert!(out.starts_with(PRELUDE) && out.contains(TEMPLATE));
        assert!(out.ends_with("\n#\"你好\"\n"));
    }

    #[test]
    fn missing_images_become_placeholders_with_warning() {
        let options = Options {
            base_dir: Some(std::env::temp_dir()),
            ..Default::default()
        };
        let out = body("段落\n\n![圖](nope/missing.png)", &options);
        assert_eq!(
            out.warnings,
            [Warning {
                line: 3,
                message: "找不到圖片：nope/missing.png".into()
            }]
        );
        assert!(out.source.contains("[圖片：圖]"));

        let out = body("![x](a.bmp) ![y](https://e.com/a.png)", &Options::default());
        assert_eq!(out.warnings.len(), 2);
    }

    #[test]
    fn inline_html() {
        assert_eq!(typst("a<BR/>b"), typst("a<br>b"));
        assert_eq!(typst("a<br>b"), "#\"a\"#linebreak()#\"b\"\n");
        assert_eq!(typst("<span>x</span> <!-- c -->"), "#\"x \"\n");
        assert_eq!(
            typst("Vec<String> 與 Option<T>"),
            "#\"Vec<String> 與 Option<T>\"\n"
        );
    }

    #[test]
    fn image_paths_are_percent_decoded() {
        assert_eq!(
            typst("![](my%20pic.png)"),
            "#align(center, image(\"my pic.png\"))\n"
        );
    }
}
