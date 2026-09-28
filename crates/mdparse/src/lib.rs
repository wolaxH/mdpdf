//! Hand-written CommonMark + GFM Markdown parser with no external dependencies.
//!
//! Parsing follows the two-phase strategy suggested by the spec: [`block`] builds the block tree, then [`inline`] parses the content of each leaf.

pub mod ast;
mod block;
#[rustfmt::skip]
mod entities;
pub mod html;
mod inline;
mod scan;
#[rustfmt::skip]
mod unicode;

pub use ast::*;
pub use scan::{is_cjk, normalize_label};

/// Parsing options. All extensions are enabled by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// GFM extensions: tables, strikethrough, task lists, footnotes.
    pub gfm: bool,
    /// CJK-friendly emphasis: a delimiter run directly next to a CJK character on its outer side
    /// may still open or close emphasis, so `這是**「重點」**這樣` becomes bold. Plain CommonMark forbids this.
    pub cjk_emphasis: bool,
    /// Math: inline `$...$`, display `$$...$$` and ```` ```math ````. The content is LaTeX source.
    pub math: bool,
    /// mdpdf extension: a line containing only `<!-- pagebreak -->` starts a new page.
    /// It is a valid HTML comment, so other Markdown tools simply hide it.
    pub page_break: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            gfm: true,
            cjk_emphasis: true,
            math: true,
            page_break: true,
        }
    }
}

impl Options {
    /// Plain CommonMark without any extension.
    pub const fn commonmark() -> Self {
        Self {
            gfm: false,
            cjk_emphasis: false,
            math: false,
            page_break: false,
        }
    }
}

/// Parse a Markdown document with the default options.
pub fn parse(input: &str) -> Document<'_> {
    parse_with(input, Options::default())
}

pub fn parse_with(input: &str, options: Options) -> Document<'_> {
    block::parse(input, options)
}
