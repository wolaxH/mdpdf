//! Markdown AST → Typst source.
//!
//! All text is emitted as Typst string literals `#"..."`, and blocks and formatting map to Typst
//! element functions (`heading`, `list`, `raw`, `strong`, ...), so Markdown content is never
//! interpreted as Typst markup.

use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::ops::Range;
use std::path::{Path, PathBuf};

use mdparse::{Align, Block, BlockKind, Cell, Inline, ListItem, is_cjk, normalize_label};

/// Helper functions called by the generated source. Unrelated to styling; always placed first.
pub const PRELUDE: &str = r#"// mdpdf helper functions
#let mdpdf-checkbox(checked) = box(
  width: 0.8em,
  height: 0.8em,
  baseline: 0.1em,
  stroke: 0.6pt + luma(80),
  radius: 1.5pt,
  if checked { place(center + horizon, text(size: 0.75em, weight: "bold", "✓")) },
)

// Title block from the front matter. A custom template may redefine this function.
#let mdpdf-title(title: none, author: (), date: none) = align(center, block(below: 2em, {
  if title != none { text(size: 1.8em, weight: "bold", title) }
  if author.len() > 0 {
    v(0.8em, weak: true)
    text(size: 1.1em, author.join(", "))
  }
  if date != none {
    v(0.5em, weak: true)
    text(fill: luma(90), date)
  }
}))
"#;

/// Added only when the document contains math: MiTeX converts LaTeX to Typst math syntax on the
/// Rust side, and the result is evaluated with MiTeX's command definitions (`mitex-scope`).
pub const MATH_PRELUDE: &str = r#"#import "@mdpdf/mitex-scope:0.2.4": mitex-scope
#let mdpdf-math(code, block: false) = math.equation(
  block: block,
  eval("$" + code + "$", scope: mitex-scope),
)
"#;

/// Typst allows 64 nested show rule applications, and each level of list, quote or inline
/// formatting uses two or three of them with the default template. Deeper nesting is flattened.
const MAX_BLOCK_NESTING: usize = 16;
const MAX_INLINE_NESTING: usize = 8;

/// Default style template, placed after [`PRELUDE`].
pub const TEMPLATE: &str = include_str!("../../../assets/template.typ");

/// Image formats Typst can read directly (by file extension).
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "svg", "pdf"];

#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Base directory for relative image paths. When set, images are checked for existence; otherwise they are not.
    pub base_dir: Option<PathBuf>,
    /// Markdown parsing options.
    pub parse: mdparse::Options,
    /// Elements forced to their fallback (indices into [`Output::fallibles`]), used to retry after a Typst layout failure.
    pub fallback: HashSet<usize>,
    /// Style settings applied on top of the template.
    pub style: Style,
    /// Custom template source used instead of [`TEMPLATE`].
    pub template: Option<String>,
}

/// Document settings emitted after the template, so they override it.
///
/// Values must already be validated: font names are emitted as string literals, but `margin` is
/// emitted verbatim as a Typst length.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Style {
    /// Body font fallback list; empty keeps the template's fonts.
    pub body_fonts: Vec<String>,
    /// Monospace font fallback list; empty keeps the template's fonts.
    pub mono_fonts: Vec<String>,
    /// A Typst paper name such as `a4` or `us-letter`.
    pub paper: Option<String>,
    /// A Typst length such as `2cm`.
    pub margin: Option<String>,
    /// Insert a table of contents after the title.
    pub toc: bool,
    /// Number headings as 1, 1.1, 1.1.1, ...
    pub number_headings: bool,
    /// Path of a `.tmTheme` syntax highlighting theme.
    pub code_theme: Option<String>,
    /// Title, authors and date shown in the title block and PDF metadata.
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub date: Option<String>,
}

impl Style {
    fn write(&self, out: &mut String) {
        let list = |out: &mut String, items: &[String]| {
            out.push('(');
            for item in items {
                push_str_literal(out, item);
                out.push_str(", ");
            }
            out.push(')');
        };
        if !self.body_fonts.is_empty() {
            out.push_str("#set text(font: ");
            list(out, &self.body_fonts);
            out.push_str(")\n");
        }
        if !self.mono_fonts.is_empty() {
            out.push_str("#show raw: set text(font: ");
            list(out, &self.mono_fonts);
            out.push_str(")\n");
        }
        if let Some(paper) = &self.paper {
            out.push_str("#set page(paper: ");
            push_str_literal(out, paper);
            out.push_str(")\n");
        }
        if let Some(margin) = &self.margin {
            writeln!(out, "#set page(margin: {margin})").unwrap();
        }
        if self.number_headings {
            out.push_str("#set heading(numbering: \"1.1\")\n");
        }
        if let Some(theme) = &self.code_theme {
            out.push_str("#set raw(theme: ");
            push_str_literal(out, theme);
            out.push_str(")\n");
        }
        if let Some(title) = &self.title {
            out.push_str("#set document(title: ");
            push_str_literal(out, title);
            out.push_str(")\n");
        }
        if !self.authors.is_empty() {
            out.push_str("#set document(author: ");
            list(out, &self.authors);
            out.push_str(")\n");
        }
        if self.title.is_some() || !self.authors.is_empty() || self.date.is_some() {
            out.push_str("#mdpdf-title(title: ");
            match &self.title {
                Some(title) => push_str_literal(out, title),
                None => out.push_str("none"),
            }
            out.push_str(", author: ");
            list(out, &self.authors);
            out.push_str(", date: ");
            match &self.date {
                Some(date) => push_str_literal(out, date),
                None => out.push_str("none"),
            }
            out.push_str(")\n");
        }
        if self.toc {
            out.push_str("#outline()\n");
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    /// Markdown source line (currently the first line of the enclosing block).
    pub line: u32,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct Output {
    pub source: String,
    pub warnings: Vec<Warning>,
    /// Every formula and image in `source`, in document order. Indices are stable across
    /// conversions of the same document, whatever [`Options::fallback`] contains.
    pub fallibles: Vec<Fallible>,
    /// Maps positions in `source` back to Markdown lines.
    pub source_map: SourceMap,
}

/// Maps byte offsets in the generated source back to Markdown source lines.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceMap {
    /// Where the converted body starts; everything before it is prelude and template.
    pub body_start: usize,
    /// (offset, line) pairs in increasing offset order: output from `offset` onwards came from `line`.
    entries: Vec<(usize, u32)>,
}

impl SourceMap {
    /// The Markdown line that produced the output at `offset`, or `None` for prelude and template code.
    pub fn line_at(&self, offset: usize) -> Option<u32> {
        if offset < self.body_start {
            return None;
        }
        let index = self.entries.partition_point(|&(start, _)| start <= offset);
        index.checked_sub(1).map(|i| self.entries[i].1)
    }

    fn mark(&mut self, offset: usize, line: u32) {
        if self.entries.last().is_none_or(|&(_, last)| last != line) {
            self.entries.push((offset, line));
        }
    }
}

/// An element that may fail when Typst lays it out (a formula or an image). After a failure,
/// its index goes into [`Options::fallback`] and the document is converted again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fallible {
    /// Byte range of the element's call in `source`.
    pub range: Range<usize>,
    /// Markdown source line.
    pub line: u32,
    pub kind: FallibleKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FallibleKind {
    /// A formula, with its LaTeX source; falls back to the raw source.
    Math(String),
    /// An image, with its URL; falls back to a placeholder box.
    Image(String),
}

/// Convert Markdown to complete Typst source (including the template).
pub fn convert(markdown: &str, options: &Options) -> Output {
    let mut prefix = String::with_capacity(PRELUDE.len() + TEMPLATE.len() + 2);
    prefix.push_str(PRELUDE);
    prefix.push('\n');
    prefix.push_str(options.template.as_deref().unwrap_or(TEMPLATE));
    prefix.push('\n');
    assemble(prefix, markdown, options)
}

/// Generate the body only, without the template ([`MATH_PRELUDE`] is still added when math is used).
pub fn body(markdown: &str, options: &Options) -> Output {
    assemble(String::new(), markdown, options)
}

fn assemble(mut source: String, markdown: &str, options: &Options) -> Output {
    let mut body = String::with_capacity(markdown.len() * 2);
    let mut warnings = Vec::new();
    let mut fallibles = Vec::new();
    let mut source_map = SourceMap::default();
    let uses_math = write_body(
        &mut body,
        &mut warnings,
        &mut fallibles,
        &mut source_map,
        markdown,
        options,
    );
    if uses_math {
        source.push_str(MATH_PRELUDE);
        source.push('\n');
    }
    options.style.write(&mut source);
    if options.style != Style::default() {
        source.push('\n');
    }
    let offset = source.len();
    for item in &mut fallibles {
        item.range = item.range.start + offset..item.range.end + offset;
    }
    source_map.body_start = offset;
    for entry in &mut source_map.entries {
        entry.0 += offset;
    }
    source.push_str(&body);
    Output {
        source,
        warnings,
        fallibles,
        source_map,
    }
}

/// Returns whether any formula was converted by MiTeX (and thus needs [`MATH_PRELUDE`]).
fn write_body(
    out: &mut String,
    warnings: &mut Vec<Warning>,
    fallibles: &mut Vec<Fallible>,
    source_map: &mut SourceMap,
    markdown: &str,
    options: &Options,
) -> bool {
    let doc = mdparse::parse_with(markdown, options.parse);
    let mut footnote_defs = HashMap::new();
    collect_footnotes(&doc.blocks, &mut footnote_defs);
    let mut writer = Writer {
        out,
        warnings,
        fallibles,
        source_map,
        options,
        line: 1,
        footnote_defs,
        footnote_ids: HashMap::new(),
        footnotes_in_progress: HashSet::new(),
        uses_math: false,
        depth: 0,
        containers: 0,
        formatting: 0,
        warned_too_deep: false,
    };
    writer.blocks(&doc.blocks);
    writer.uses_math
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
    fallibles: &'o mut Vec<Fallible>,
    source_map: &'o mut SourceMap,
    /// Whether any formula was converted by MiTeX.
    uses_math: bool,
    /// Nesting depth of the current block; the top level is 1.
    depth: usize,
    /// Enclosing lists and block quotes.
    containers: usize,
    /// Enclosing emphasis, strong, strikethrough and links.
    formatting: usize,
    warned_too_deep: bool,
    options: &'o Options,
    /// First line of the block being processed, for warnings.
    line: u32,
    /// Footnote definitions (normalized label → content).
    footnote_defs: HashMap<String, &'d [Block<'a>]>,
    /// Footnotes already emitted and their Typst label ids; repeated references point to the same footnote.
    footnote_ids: HashMap<String, usize>,
    /// Footnotes being emitted, to prevent infinite recursion when a footnote references itself.
    footnotes_in_progress: HashSet<String>,
}

impl Writer<'_, '_, '_> {
    fn warn(&mut self, message: String) {
        self.warn_at(self.line, message);
    }

    fn warn_at(&mut self, line: u32, message: String) {
        self.warnings.push(Warning { line, message });
    }

    fn blocks_ref(&mut self, blocks: &[&Block]) {
        for (i, block) in blocks.iter().enumerate() {
            if i > 0 {
                self.out.push('\n');
            }
            self.blocks(std::slice::from_ref(*block));
        }
    }

    /// Emit a sequence of blocks separated by blank lines. Every block ends with a newline.
    fn blocks(&mut self, blocks: &[Block]) {
        self.depth += 1;
        let mut first = true;
        for block in blocks {
            // Typst allows page breaks only at the top level; markers inside lists, quotes or footnotes are ignored
            if matches!(block.kind, BlockKind::PageBreak) && self.depth > 1 {
                self.line = block.span.line;
                self.warn("換頁標記只能用在最外層（不能在清單、引言或腳註中），已忽略".into());
                continue;
            }
            // Raw HTML is not rendered; footnote definitions are emitted where referenced
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
            self.source_map.mark(self.out.len(), self.line);
            self.block(block);
        }
        self.depth -= 1;
    }

    fn block(&mut self, block: &Block) {
        match &block.kind {
            BlockKind::Paragraph(inlines) if is_image_paragraph(inlines) => {
                // A paragraph with only images: each image becomes a centered block
                for inline in inlines {
                    if let Inline::Image { url, alt, line, .. } = inline {
                        self.image(url, alt, *line, true);
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
            BlockKind::BlockQuote(children) if self.containers >= MAX_BLOCK_NESTING => {
                self.too_deep();
                self.blocks(children);
            }
            BlockKind::BlockQuote(children) => {
                self.containers += 1;
                self.out.push_str("#quote(block: true)[\n");
                self.blocks(children);
                self.out.push_str("]\n");
                self.containers -= 1;
            }
            BlockKind::List { items, .. } if self.containers >= MAX_BLOCK_NESTING => {
                self.too_deep();
                let blocks: Vec<&Block> = items.iter().flat_map(|item| &item.blocks).collect();
                self.blocks_ref(&blocks);
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
                // Task lists replace the bullet with a checkbox
                if ordered.is_none() && items.iter().any(|i| i.task.is_some()) {
                    self.out.push_str(" marker: [],");
                }
                self.out.push('\n');
                self.containers += 1;
                for item in items {
                    self.list_item(item);
                }
                self.containers -= 1;
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
            BlockKind::MathBlock(tex) => {
                self.math(tex, self.line, true);
                self.out.push('\n');
            }
            BlockKind::ThematicBreak => self.out.push_str("#line(length: 100%)\n"),
            // weak: no extra blank page when already at the start of a page
            BlockKind::PageBreak => self.out.push_str("#pagebreak(weak: true)\n"),
            BlockKind::Table { align, head, rows } => self.table(align, head, rows),
        }
    }

    fn table(&mut self, align: &[Align], head: &[Cell], rows: &[Vec<Cell>]) {
        writeln!(self.out, "#table(\n  columns: {},", align.len()).unwrap();
        let align: Vec<_> = align
            .iter()
            .map(|a| match a {
                // Explicit left alignment so an outer center alignment does not affect the cells
                Align::None | Align::Left => "left",
                Align::Center => "center",
                Align::Right => "right",
            })
            .collect();
        // A single column needs a trailing comma, otherwise `(left)` is not an array
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
        // Merge adjacent text into a single string literal
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
                    self.wrapped("#emph[", children);
                }
                Inline::Strong(children) => {
                    self.flush_text(&mut run);
                    self.wrapped("#strong[", children);
                }
                Inline::Strike(children) => {
                    self.flush_text(&mut run);
                    self.wrapped("#strike[", children);
                }
                Inline::Math { tex, display, line } => {
                    self.flush_text(&mut run);
                    self.math(tex, *line, *display);
                }
                Inline::FootnoteRef(label) => {
                    self.flush_text(&mut run);
                    self.footnote(label);
                }
                Inline::Link { url, content, .. } => {
                    self.flush_text(&mut run);
                    if url.is_empty() {
                        // Typst rejects an empty link target; emit only the text
                        self.inlines(content);
                    } else {
                        let mut open = String::from("#link(");
                        push_str_literal(&mut open, url);
                        open.push_str(")[");
                        self.wrapped(&open, content);
                    }
                }
                Inline::Image { url, alt, line, .. } => {
                    self.flush_text(&mut run);
                    self.image(url, alt, *line, false);
                }
                Inline::Html(html) => match html_tag_name(html) {
                    Some(name) if name.eq_ignore_ascii_case("br") => {
                        self.flush_text(&mut run);
                        self.out.push_str("#linebreak()");
                    }
                    // Standard HTML tags are not rendered (the text between them is kept)
                    Some(name) if is_html_element(name) => {}
                    // A "tag" that is not an HTML element is usually text such as `Vec<T>`; emit it verbatim
                    Some(_) => run.push_str(html),
                    // Comments, processing instructions, etc.
                    None => {}
                },
            }
        }
        self.flush_text(&mut run);
    }

    /// Convert LaTeX to Typst math with MiTeX. On conversion failure, or when forced to fall back, show the raw source in monospace.
    fn math(&mut self, tex: &str, line: u32, block: bool) {
        let index = self.fallibles.len();
        let start = self.out.len();
        self.source_map.mark(start, line);
        let converted = if self.options.fallback.contains(&index) {
            None
        } else {
            match mitex::convert_math(tex, None) {
                Ok(code) => Some(space_cases(&code)),
                Err(err) => {
                    let err = err.strip_prefix("error: ").unwrap_or(&err);
                    self.warn_at(line, format!("無法轉換公式 `{tex}`：{err}，改以原文顯示"));
                    None
                }
            }
        };
        match converted {
            Some(code) => {
                self.uses_math = true;
                self.out.push_str("#mdpdf-math(");
                push_str_literal(self.out, &code);
                self.out
                    .push_str(if block { ", block: true)" } else { ")" });
            }
            None => {
                self.out
                    .push_str(if block { "#raw(block: true, " } else { "#raw(" });
                push_str_literal(self.out, tex);
                self.out.push(')');
            }
        }
        self.fallibles.push(Fallible {
            range: start..self.out.len(),
            line,
            kind: FallibleKind::Math(tex.to_string()),
        });
    }

    /// Emit `children` inside `open` ... `]`, or without the wrapper once formatting is nested too deeply.
    fn wrapped(&mut self, open: &str, children: &[Inline]) {
        if self.formatting >= MAX_INLINE_NESTING {
            self.too_deep();
            self.inlines(children);
            return;
        }
        self.formatting += 1;
        self.out.push_str(open);
        self.inlines(children);
        self.out.push(']');
        self.formatting -= 1;
    }

    /// Warn once that nesting beyond the limits was flattened.
    fn too_deep(&mut self) {
        if !self.warned_too_deep {
            self.warned_too_deep = true;
            self.warn("巢狀層數過深，更深層的清單、引言或文字格式已省略".into());
        }
    }

    /// The first reference emits the full footnote with a label; later references point to it.
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

    fn image(&mut self, url: &str, alt: &[Inline], line: u32, block: bool) {
        let index = self.fallibles.len();
        let start = self.out.len();
        self.source_map.mark(start, line);
        let alt = mdparse::plain_text(alt);
        let path = if self.options.fallback.contains(&index) {
            // Already reported when Typst failed to load it
            None
        } else {
            self.check_image(url)
                .map_err(|message| self.warn_at(line, message))
                .ok()
        };
        match path {
            Some(path) => {
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
            None => {
                // Placeholder box: show the alt text (or the path when there is none)
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
            }
        }
        self.fallibles.push(Fallible {
            range: start..self.out.len(),
            line,
            kind: FallibleKind::Image(url.to_string()),
        });
    }

    /// Check that an image is usable and return the path to hand to Typst.
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

/// Standard HTML element names (sorted).
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

/// Name of an open or closing tag; `None` for comments and other raw HTML.
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

/// Add spacing between the values and conditions of `cases(...)`.
///
/// MiTeX turns LaTeX `&` into Typst alignment points, but Typst's `cases` leaves no space at an
/// alignment point, so `x^2 & x \ge 0` would render as "x² x ≥ 0". Only the `&` at the top level of
/// `cases(` is changed; escapes (`\(`, `\&`, ...) and nested parentheses are left alone.
fn space_cases(code: &str) -> String {
    if !code.contains("cases(") {
        return code.to_string();
    }
    let mut out = String::with_capacity(code.len() + 16);
    // Whether each open parenthesis holds the arguments of cases
    let mut stack: Vec<bool> = Vec::new();
    let mut chars = code.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => {
                out.push(c);
                if let Some((_, next)) = chars.next() {
                    out.push(next);
                }
                continue;
            }
            '(' => {
                let before = &code[..i];
                let is_cases = before.ends_with("cases")
                    && !before[..before.len() - 5]
                        .chars()
                        .next_back()
                        .is_some_and(|p| p.is_alphanumeric() && p != 'r');
                stack.push(is_cases);
            }
            ')' => {
                stack.pop();
            }
            '&' if stack.last() == Some(&true) => {
                out.push_str("& quad");
                continue;
            }
            _ => {}
        }
        out.push(c);
    }
    out
}

/// Whether a paragraph consists only of images (with only line breaks or spaces between them).
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
        // Code spans, images, etc. count as non-CJK
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

/// Decode `%XX` in a URL, for local file paths.
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

/// Whether a soft line break in the source should become a space in the output.
///
/// No space is inserted when either side is a CJK character; otherwise every line break in a
/// Chinese paragraph would add a space. A neighbor that is not text (e.g. a code span) counts as non-CJK.
pub fn soft_break(prev: Option<char>, next: Option<char>) -> bool {
    !(prev.is_some_and(is_cjk) || next.is_some_and(is_cjk))
}

/// Emit plain text as a Typst string literal `#"..."`.
///
/// Only `"` and `\` are special inside a string, so markup characters in the text such as `*`,
/// `_`, `#`, `$` and `@` are never interpreted by Typst, which rules out injection entirely.
pub fn push_text(out: &mut String, text: &str) {
    out.push('#');
    push_str_literal(out, text);
}

/// Emit a Typst string literal (including the quotes).
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
    fn style_settings() {
        let style = Style {
            body_fonts: vec!["Libertinus Serif".into(), "Noto Sans TC".into()],
            mono_fonts: vec!["Fira Code".into()],
            paper: Some("us-letter".into()),
            margin: Some("2cm".into()),
            toc: true,
            number_headings: true,
            code_theme: Some("/t/x.tmTheme".into()),
            title: Some("報告 \"Q3\"".into()),
            authors: vec!["A".into(), "B".into()],
            date: None,
        };
        let out = body(
            "text",
            &Options {
                style,
                ..Default::default()
            },
        )
        .source;
        let expected = [
            "#set text(font: (\"Libertinus Serif\", \"Noto Sans TC\", ))",
            "#show raw: set text(font: (\"Fira Code\", ))",
            "#set page(paper: \"us-letter\")",
            "#set page(margin: 2cm)",
            "#set heading(numbering: \"1.1\")",
            "#set raw(theme: \"/t/x.tmTheme\")",
            "#set document(title: \"報告 \\\"Q3\\\"\")",
            "#set document(author: (\"A\", \"B\", ))",
            "#mdpdf-title(title: \"報告 \\\"Q3\\\"\", author: (\"A\", \"B\", ), date: none)",
            "#outline()",
        ];
        for line in expected {
            assert!(out.contains(line), "missing {line:?} in:\n{out}");
        }
        assert!(out.ends_with("#\"text\"\n"));

        // Default style emits nothing
        assert_eq!(typst("text"), "#\"text\"\n");
    }

    #[test]
    fn warnings_point_at_the_exact_line() {
        let md = "段落第一行\n第二行 ![a](missing.png)\n第三行 $\\foo$\n\n| a |\n| - |\n| x |\n| ![b](gone.png) |\n";
        let out = body(
            md,
            &Options {
                base_dir: Some(std::env::temp_dir()),
                ..Default::default()
            },
        );
        let lines: Vec<u32> = out.warnings.iter().map(|w| w.line).collect();
        assert_eq!(lines, [2, 3, 8], "{:#?}", out.warnings);
        assert_eq!(out.fallibles[1].line, 3);
    }

    #[test]
    fn source_map_points_back_to_markdown() {
        let md = "# 標題\n\n段落\n第二行 ![a](x.png) 之後\n\n- 清單\n";
        let out = convert(md, &Options::default());
        let map = &out.source_map;
        assert_eq!(
            map.line_at(0),
            None,
            "prelude and template have no Markdown line"
        );
        let at = |needle: &str| map.line_at(out.source.find(needle).unwrap());
        assert_eq!(at("#heading"), Some(1));
        assert_eq!(at("#\"段落"), Some(3));
        assert_eq!(at("#box(image"), Some(4));
        assert_eq!(at("#list"), Some(6));
    }

    #[test]
    fn page_breaks() {
        let out = body(
            "一\n\n<!-- pagebreak -->\n\n二\n\n- 項目\n\n  <!-- pagebreak -->\n",
            &Options::default(),
        );
        assert_eq!(out.source.matches("#pagebreak(weak: true)").count(), 1);
        assert_eq!(out.warnings.len(), 1);
        assert_eq!(out.warnings[0].line, 9);
    }

    #[test]
    fn cases_get_spacing() {
        assert_eq!(
            space_cases("cases( x & x >= 0 , - x & x < 0 )"),
            "cases( x & quad x >= 0 , - x & quad x < 0 )"
        );
        assert_eq!(space_cases("rcases(a & b)"), "rcases(a & quad b)");
        // & inside nested parentheses or escapes is left alone
        assert_eq!(
            space_cases("cases(f(a & b) & \\& c)"),
            "cases(f(a & b) & quad \\& c)"
        );
        assert_eq!(space_cases("pmatrix(a & b)"), "pmatrix(a & b)");
        assert_eq!(space_cases("mycases(a & b)"), "mycases(a & b)");
    }

    #[test]
    fn image_paths_are_percent_decoded() {
        assert_eq!(
            typst("![](my%20pic.png)"),
            "#align(center, image(\"my pic.png\"))\n"
        );
    }
}
