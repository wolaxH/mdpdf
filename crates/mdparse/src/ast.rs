//! Markdown abstract syntax tree.
//!
//! Text borrows from the input as `Cow<'a, str>`; a new string is allocated only when the
//! text cannot be sliced directly, e.g. after unescaping or for lines spanning container prefixes.

use std::borrow::Cow;
use std::collections::HashMap;

/// A position in the input (1-based; columns count characters).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Span {
    pub line: u32,
    pub col: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Document<'a> {
    pub blocks: Vec<Block<'a>>,
    /// Link reference definitions, keyed by normalized label.
    pub link_defs: HashMap<String, LinkDef<'a>>,
    /// Raw YAML of the front matter block, without the `---` delimiters.
    pub front_matter: Option<Cow<'a, str>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LinkDef<'a> {
    pub url: Cow<'a, str>,
    pub title: Option<Cow<'a, str>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block<'a> {
    pub kind: BlockKind<'a>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BlockKind<'a> {
    Heading {
        level: u8,
        content: Vec<Inline<'a>>,
    },
    Paragraph(Vec<Inline<'a>>),
    BlockQuote(Vec<Block<'a>>),
    List {
        /// Start number of an ordered list; `None` for a bullet list.
        ordered: Option<u32>,
        tight: bool,
        items: Vec<ListItem<'a>>,
    },
    CodeBlock {
        /// First word of the info string.
        lang: Option<Cow<'a, str>>,
        code: Cow<'a, str>,
    },
    /// Raw HTML block: recognized but not rendered.
    Html(Cow<'a, str>),
    ThematicBreak,
    /// Page break (mdpdf extension, see [`crate::Options::page_break`]).
    PageBreak,
    /// Display math; the content is LaTeX source.
    MathBlock(Cow<'a, str>),
    /// GFM table. The header and every row have exactly `align.len()` cells.
    Table {
        align: Vec<Align>,
        head: Vec<Cell<'a>>,
        rows: Vec<Vec<Cell<'a>>>,
    },
    /// Footnote definition. `label` is the raw label; compare via [`crate::normalize_label`].
    FootnoteDef {
        label: Cow<'a, str>,
        blocks: Vec<Block<'a>>,
    },
}

/// Column alignment of a table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    None,
    Left,
    Center,
    Right,
}

/// Content of a table cell.
pub type Cell<'a> = Vec<Inline<'a>>;

#[derive(Debug, Clone, PartialEq)]
pub struct ListItem<'a> {
    pub blocks: Vec<Block<'a>>,
    pub span: Span,
    /// GFM task list item: `Some(true)` when checked.
    pub task: Option<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Inline<'a> {
    Text(Cow<'a, str>),
    Code(Cow<'a, str>),
    Emph(Vec<Inline<'a>>),
    Strong(Vec<Inline<'a>>),
    /// GFM strikethrough.
    Strike(Vec<Inline<'a>>),
    /// Footnote reference with its raw label. Only produced when a matching definition exists.
    FootnoteRef(Cow<'a, str>),
    /// Link. `url` is unescaped with entities decoded, but not percent-encoded.
    Link {
        url: Cow<'a, str>,
        title: Option<Cow<'a, str>>,
        content: Vec<Inline<'a>>,
    },
    /// Image. `alt` keeps its inline structure; use [`plain_text`] for plain text.
    Image {
        url: Cow<'a, str>,
        title: Option<Cow<'a, str>>,
        alt: Vec<Inline<'a>>,
        /// Source line of the image, for diagnostics.
        line: u32,
    },
    /// Raw inline HTML: recognized but not rendered.
    Html(Cow<'a, str>),
    /// Math; the content is LaTeX source. `display` is set for `$$...$$` inside a paragraph.
    Math {
        tex: Cow<'a, str>,
        display: bool,
        /// Source line of the formula, for diagnostics.
        line: u32,
    },
    SoftBreak,
    HardBreak,
}

impl Inline<'_> {
    pub fn into_owned(self) -> Inline<'static> {
        let all = |v: Vec<Inline>| v.into_iter().map(Inline::into_owned).collect();
        match self {
            Inline::Text(s) => Inline::Text(owned(s)),
            Inline::Code(s) => Inline::Code(owned(s)),
            Inline::Emph(v) => Inline::Emph(all(v)),
            Inline::Strong(v) => Inline::Strong(all(v)),
            Inline::Strike(v) => Inline::Strike(all(v)),
            Inline::FootnoteRef(s) => Inline::FootnoteRef(owned(s)),
            Inline::Link {
                url,
                title,
                content,
            } => Inline::Link {
                url: owned(url),
                title: title.map(owned),
                content: all(content),
            },
            Inline::Image {
                url,
                title,
                alt,
                line,
            } => Inline::Image {
                url: owned(url),
                title: title.map(owned),
                alt: all(alt),
                line,
            },
            Inline::Html(s) => Inline::Html(owned(s)),
            Inline::Math { tex, display, line } => Inline::Math {
                tex: owned(tex),
                display,
                line,
            },
            Inline::SoftBreak => Inline::SoftBreak,
            Inline::HardBreak => Inline::HardBreak,
        }
    }
}

/// Plain text of inline content (e.g. for image alt): formatting dropped, breaks become spaces.
pub fn plain_text(inlines: &[Inline]) -> String {
    fn walk(inlines: &[Inline], out: &mut String) {
        for inline in inlines {
            match inline {
                Inline::Text(s) | Inline::Code(s) | Inline::Math { tex: s, .. } => out.push_str(s),
                Inline::Emph(v) | Inline::Strong(v) | Inline::Strike(v) => walk(v, out),
                Inline::Link { content, .. } => walk(content, out),
                Inline::Image { alt, .. } => walk(alt, out),
                Inline::Html(_) | Inline::FootnoteRef(_) => {}
                Inline::SoftBreak | Inline::HardBreak => out.push(' '),
            }
        }
    }
    let mut out = String::new();
    walk(inlines, &mut out);
    out
}

impl LinkDef<'_> {
    pub fn into_owned(self) -> LinkDef<'static> {
        LinkDef {
            url: owned(self.url),
            title: self.title.map(owned),
        }
    }
}

pub(crate) fn owned(s: Cow<'_, str>) -> Cow<'static, str> {
    Cow::Owned(s.into_owned())
}
