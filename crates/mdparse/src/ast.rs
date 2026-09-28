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
    /// 換頁（mdpdf 擴充，見 [`crate::Options::page_break`]）。
    PageBreak,
    /// 區塊數學公式，內容為 LaTeX 原文。
    MathBlock(Cow<'a, str>),
    /// GFM 表格。表頭與每一列的儲存格數都等於 `align.len()`。
    Table {
        align: Vec<Align>,
        head: Vec<Cell<'a>>,
        rows: Vec<Vec<Cell<'a>>>,
    },
    /// 腳註定義。`label` 為原始標籤，比對時以 [`crate::normalize_label`] 正規化。
    FootnoteDef {
        label: Cow<'a, str>,
        blocks: Vec<Block<'a>>,
    },
}

/// 表格欄位的對齊方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    None,
    Left,
    Center,
    Right,
}

/// 表格儲存格的內容。
pub type Cell<'a> = Vec<Inline<'a>>;

#[derive(Debug, Clone, PartialEq)]
pub struct ListItem<'a> {
    pub blocks: Vec<Block<'a>>,
    pub span: Span,
    /// GFM 任務清單項目：`Some(true)` 為已勾選。
    pub task: Option<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Inline<'a> {
    Text(Cow<'a, str>),
    Code(Cow<'a, str>),
    Emph(Vec<Inline<'a>>),
    Strong(Vec<Inline<'a>>),
    /// GFM 刪除線。
    Strike(Vec<Inline<'a>>),
    /// 腳註參照，內容為原始標籤。只有對應的定義存在時才會產生。
    FootnoteRef(Cow<'a, str>),
    /// 連結。`url` 已處理跳脫與字元參照，但未做百分比編碼。
    Link {
        url: Cow<'a, str>,
        title: Option<Cow<'a, str>>,
        content: Vec<Inline<'a>>,
    },
    /// 圖片。`alt` 保留 inline 結構，需要純文字時用 [`plain_text`]。
    Image {
        url: Cow<'a, str>,
        title: Option<Cow<'a, str>>,
        alt: Vec<Inline<'a>>,
    },
    /// 行內原始 HTML：只辨識，不渲染。
    Html(Cow<'a, str>),
    /// 數學公式，內容為 LaTeX 原文。`display` 為段落中的 `$$...$$`。
    Math {
        tex: Cow<'a, str>,
        display: bool,
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
            Inline::Image { url, title, alt } => Inline::Image {
                url: owned(url),
                title: title.map(owned),
                alt: all(alt),
            },
            Inline::Html(s) => Inline::Html(owned(s)),
            Inline::Math { tex, display } => Inline::Math {
                tex: owned(tex),
                display,
            },
            Inline::SoftBreak => Inline::SoftBreak,
            Inline::HardBreak => Inline::HardBreak,
        }
    }
}

/// inline 內容的純文字（用於圖片 alt 等）：去掉格式，換行轉為空白。
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
