//! 自製 CommonMark + GFM Markdown parser，沒有外部相依。
//!
//! 採用規格建議的兩階段解析：[`block`] 先建立區塊樹，再由 [`inline`] 逐段解析行內內容。

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

/// 解析選項。預設開啟所有擴充。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// GFM 擴充：表格、刪除線、任務清單、腳註。
    pub gfm: bool,
    /// CJK 寬鬆強調：強調符號外側緊鄰中日韓文字時，也視為可以開啟／關閉強調，
    /// 讓 `這是**「重點」**這樣` 能正確變成粗體。標準 CommonMark 不允許。
    pub cjk_emphasis: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            gfm: true,
            cjk_emphasis: true,
        }
    }
}

impl Options {
    /// 純 CommonMark，不含任何擴充。
    pub const fn commonmark() -> Self {
        Self {
            gfm: false,
            cjk_emphasis: false,
        }
    }
}

/// 以預設選項解析 Markdown 文件。
pub fn parse(input: &str) -> Document<'_> {
    parse_with(input, Options::default())
}

pub fn parse_with(input: &str, options: Options) -> Document<'_> {
    block::parse(input, options)
}
