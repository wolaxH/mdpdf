//! 自製 CommonMark + GFM Markdown parser，沒有外部相依。
//!
//! 採用規格建議的兩階段解析：[`block`] 先建立區塊樹，再由 [`inline`] 逐段解析行內內容。

pub mod ast;
mod block;
pub mod html;
mod inline;
mod scan;

pub use ast::*;

/// 解析 Markdown 文件。
pub fn parse(input: &str) -> Document<'_> {
    block::parse(input)
}
