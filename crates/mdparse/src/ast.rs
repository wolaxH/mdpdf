//! Markdown 抽象語法樹。
//!
//! 文字以 `Cow<'a, str>` 引用原始輸入；只有跳脫字元、跨越容器前綴的多行內容等
//! 無法直接切片的情況才配置新字串。

use std::borrow::Cow;
use std::collections::HashMap;

/// 原始輸入中的位置（皆從 1 起算，欄位以字元計）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Span {
    pub line: u32,
    pub col: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Document<'a> {
    pub blocks: Vec<Block<'a>>,
    /// 連結參照定義，key 為正規化後的標籤。
    pub link_defs: HashMap<String, LinkDef<'a>>,
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
        /// 有序清單的起始編號；`None` 為無序清單。
        ordered: Option<u32>,
        tight: bool,
        items: Vec<ListItem<'a>>,
    },
    CodeBlock {
        /// info string 的第一個詞。
        lang: Option<Cow<'a, str>>,
        code: Cow<'a, str>,
    },
    /// 原始 HTML 區塊：只辨識，不渲染。
    Html(Cow<'a, str>),
    ThematicBreak,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ListItem<'a> {
    pub blocks: Vec<Block<'a>>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Inline<'a> {
    Text(Cow<'a, str>),
    Code(Cow<'a, str>),
    SoftBreak,
    HardBreak,
}

impl Inline<'_> {
    pub fn into_owned(self) -> Inline<'static> {
        match self {
            Inline::Text(s) => Inline::Text(owned(s)),
            Inline::Code(s) => Inline::Code(owned(s)),
            Inline::SoftBreak => Inline::SoftBreak,
            Inline::HardBreak => Inline::HardBreak,
        }
    }
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
