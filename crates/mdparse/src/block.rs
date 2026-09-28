//! 區塊階段：逐行掃描，建立區塊樹。
//!
//! 採用 CommonMark 規格附錄建議的演算法：維護一條「開啟中的容器」路徑，每一行先嘗試
//! 讓既有容器延續，再尋找新區塊的開頭，最後把剩下的文字交給葉節點。延續失敗但仍可
//! 接到開啟中段落的行即為 lazy continuation。
//!
//! 解析期間節點放在 arena（`Vec<Node>`）中，結束後再轉成 [`crate::ast`] 的樹。

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use crate::ast::{Align, Block, BlockKind, Cell, Document, LinkDef, ListItem, Span};
use crate::inline::Ctx;
use crate::{Options, inline, scan};

const CODE_INDENT: usize = 4;
const ROOT: usize = 0;

pub fn parse(input: &str, options: Options) -> Document<'_> {
    let mut parser = Parser::new(input, options);
    let mut rest = input;
    while !rest.is_empty() {
        let end = rest.find(['\n', '\r']).unwrap_or(rest.len());
        parser.incorporate_line(&rest[..end]);
        let eol = match rest.as_bytes().get(end) {
            Some(b'\r') if rest.as_bytes().get(end + 1) == Some(&b'\n') => 2,
            Some(_) => 1,
            None => 0,
        };
        rest = &rest[end + eol..];
    }
    parser.finish()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListType {
    Bullet(u8),
    /// 有序清單，帶分隔符 `.` 或 `)`。
    Ordered(u8),
}

#[derive(Debug, Clone, Copy)]
struct ListData {
    ty: ListType,
    start: u32,
    /// 標記相對於容器內容起點的縮排欄數。
    marker_offset: usize,
    /// 標記寬度加上其後的空白，即內容相對於標記的欄數。
    padding: usize,
}

#[derive(Debug, Clone, Copy)]
struct Fence {
    ch: u8,
    len: usize,
    /// 開頭 fence 的縮排，內容行會移除至多這麼多的空白。
    offset: usize,
}

#[derive(Debug)]
enum Kind {
    Document,
    BlockQuote,
    List {
        data: ListData,
        tight: bool,
    },
    Item {
        data: ListData,
    },
    Paragraph,
    Heading {
        level: u8,
    },
    ThematicBreak,
    CodeBlock {
        fence: Option<Fence>,
    },
    Html {
        kind: u8,
    },
    /// GFM 表格。`lines[0]` 是表頭，`lines[1]` 是分隔列（略過），其後為資料列。
    Table {
        align: Vec<Align>,
    },
    /// 腳註定義，標籤存在 `content`。
    FootnoteDef,
    /// 跨行的 `$$` 數學區塊。`offset` 是開頭 `$$` 的縮排，內容行會移除至多這麼多空白。
    MathBlock {
        offset: usize,
    },
}

impl Kind {
    fn accepts_lines(&self) -> bool {
        matches!(
            self,
            Kind::Paragraph
                | Kind::CodeBlock { .. }
                | Kind::Html { .. }
                | Kind::Table { .. }
                | Kind::MathBlock { .. }
        )
    }

    fn can_contain(&self, child: &Kind) -> bool {
        match self {
            Kind::Document | Kind::BlockQuote | Kind::Item { .. } | Kind::FootnoteDef => {
                !matches!(child, Kind::Item { .. })
            }
            Kind::List { .. } => matches!(child, Kind::Item { .. }),
            _ => false,
        }
    }
}

/// 去除容器前綴後的一行內容。`pad` 是部分消耗的 tab 需補回的空白數。
#[derive(Debug, Clone, Copy)]
struct Line<'a> {
    pad: usize,
    text: &'a str,
}

#[derive(Debug)]
struct Node<'a> {
    kind: Kind,
    parent: usize,
    children: Vec<usize>,
    open: bool,
    start: Span,
    end_line: u32,
    lines: Vec<Line<'a>>,
    /// 段落與標題的最終文字內容；腳註定義的標籤。
    content: Cow<'a, str>,
    /// 程式碼區塊的語言。
    lang: Option<Cow<'a, str>>,
}

enum Continue {
    Matched,
    Failed,
    /// 這一行已被完全處理（例如 fence 結尾）。
    Consumed,
}

enum Start {
    None,
    Container,
    Leaf,
}

struct Parser<'a> {
    input: &'a str,
    options: Options,
    nodes: Vec<Node<'a>>,
    link_defs: HashMap<String, LinkDef<'a>>,
    /// 已定義的腳註標籤（正規化後）。
    footnotes: HashSet<String>,

    tip: usize,
    old_tip: usize,
    last_matched: usize,
    all_closed: bool,

    line: &'a str,
    line_number: u32,
    offset: usize,
    column: usize,
    next_nonspace: usize,
    next_nonspace_column: usize,
    indent: usize,
    indented: bool,
    blank: bool,
    partially_consumed_tab: bool,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str, options: Options) -> Self {
        let root = Node::new(Kind::Document, ROOT, Span { line: 1, col: 1 });
        Self {
            input,
            options,
            nodes: vec![root],
            link_defs: HashMap::new(),
            footnotes: HashSet::new(),
            tip: ROOT,
            old_tip: ROOT,
            last_matched: ROOT,
            all_closed: true,
            line: "",
            line_number: 0,
            offset: 0,
            column: 0,
            next_nonspace: 0,
            next_nonspace_column: 0,
            indent: 0,
            indented: false,
            blank: false,
            partially_consumed_tab: false,
        }
    }

    // ---- 行內游標 ----

    fn peek(&self, pos: usize) -> Option<u8> {
        self.line.as_bytes().get(pos).copied()
    }

    fn find_next_nonspace(&mut self) {
        let bytes = self.line.as_bytes();
        let mut i = self.offset;
        let mut cols = self.column;
        while let Some(&c) = bytes.get(i) {
            match c {
                b' ' => cols += 1,
                b'\t' => cols += 4 - cols % 4,
                _ => break,
            }
            i += 1;
        }
        self.blank = i == bytes.len();
        self.next_nonspace = i;
        self.next_nonspace_column = cols;
        self.indent = cols - self.column;
        self.indented = self.indent >= CODE_INDENT;
    }

    /// 前進 `count` 個字元；`columns` 為真時以欄計，tab 可以只消耗一部分。
    fn advance_offset(&mut self, mut count: usize, columns: bool) {
        let bytes = self.line.as_bytes();
        while count > 0 && self.offset < bytes.len() {
            if bytes[self.offset] == b'\t' {
                let to_tab = 4 - self.column % 4;
                if columns {
                    self.partially_consumed_tab = to_tab > count;
                    let advance = to_tab.min(count);
                    self.column += advance;
                    if !self.partially_consumed_tab {
                        self.offset += 1;
                    }
                    count -= advance;
                } else {
                    self.partially_consumed_tab = false;
                    self.column += to_tab;
                    self.offset += 1;
                    count -= 1;
                }
            } else {
                self.partially_consumed_tab = false;
                self.offset += 1;
                self.column += 1;
                count -= 1;
            }
        }
    }

    fn advance_next_nonspace(&mut self) {
        self.offset = self.next_nonspace;
        self.column = self.next_nonspace_column;
        self.partially_consumed_tab = false;
    }

    fn advance_to_end(&mut self) {
        self.advance_offset(self.line.len() - self.offset, false);
    }

    // ---- 樹操作 ----

    fn span_at(&self, offset: usize) -> Span {
        let col = self.line[..offset].chars().count() as u32 + 1;
        Span {
            line: self.line_number,
            col,
        }
    }

    fn add_line(&mut self) {
        let mut pad = 0;
        if self.partially_consumed_tab {
            self.offset += 1;
            pad = 4 - self.column % 4;
        }
        let text = &self.line[self.offset..];
        self.nodes[self.tip].lines.push(Line { pad, text });
    }

    fn add_child(&mut self, kind: Kind, offset: usize) -> usize {
        while !self.nodes[self.tip].kind.can_contain(&kind) {
            self.finalize(self.tip, self.line_number - 1);
        }
        let id = self.nodes.len();
        self.nodes
            .push(Node::new(kind, self.tip, self.span_at(offset)));
        self.nodes[self.tip].children.push(id);
        self.tip = id;
        id
    }

    fn close_unmatched_blocks(&mut self) {
        if !self.all_closed {
            while self.old_tip != self.last_matched {
                let parent = self.nodes[self.old_tip].parent;
                self.finalize(self.old_tip, self.line_number - 1);
                self.old_tip = parent;
            }
            self.all_closed = true;
        }
    }

    // ---- 逐行處理 ----

    fn incorporate_line(&mut self, line: &'a str) {
        self.line = line;
        self.line_number += 1;
        self.offset = 0;
        self.column = 0;
        self.blank = false;
        self.partially_consumed_tab = false;
        self.old_tip = self.tip;

        // 1. 讓開啟中的容器依序嘗試延續。
        let mut container = ROOT;
        let mut all_matched = true;
        while let Some(&last) = self.nodes[container].children.last() {
            if !self.nodes[last].open {
                break;
            }
            container = last;
            self.find_next_nonspace();
            match self.continue_block(container) {
                Continue::Matched => {}
                Continue::Failed => {
                    all_matched = false;
                    break;
                }
                Continue::Consumed => return,
            }
        }
        if !all_matched {
            container = self.nodes[container].parent;
        }
        self.all_closed = container == self.old_tip;
        self.last_matched = container;

        // 2. 尋找新區塊的開頭。
        // 段落與表格即使延續成功，這一行仍可能開始新區塊
        let mut matched_leaf = {
            let kind = &self.nodes[container].kind;
            !matches!(kind, Kind::Paragraph | Kind::Table { .. }) && kind.accepts_lines()
        };
        let (gfm, math) = (self.options.gfm, self.options.math);
        while !matched_leaf {
            self.find_next_nonspace();
            let maybe_special = self.peek(self.next_nonspace).is_some_and(|c| {
                b"#`~*+_=<>-".contains(&c)
                    || c.is_ascii_digit()
                    || (gfm && b"|:[".contains(&c))
                    || (math && c == b'$')
            });
            if !self.indented && !maybe_special {
                self.advance_next_nonspace();
                break;
            }
            match self.block_start(container) {
                Start::Container => container = self.tip,
                Start::Leaf => {
                    container = self.tip;
                    matched_leaf = true;
                }
                Start::None => {
                    self.advance_next_nonspace();
                    break;
                }
            }
        }

        // 3. 剩下的文字加到適當的區塊。
        if !self.all_closed && !self.blank && matches!(self.nodes[self.tip].kind, Kind::Paragraph) {
            // lazy continuation
            self.add_line();
        } else {
            self.close_unmatched_blocks();
            if self.nodes[container].kind.accepts_lines() {
                self.add_line();
                if let Kind::Html { kind: kind @ 1..=5 } = self.nodes[container].kind
                    && scan::html_block_end(kind, &self.line[self.offset..])
                {
                    self.finalize(container, self.line_number);
                }
            } else if self.offset < line.len() && !self.blank {
                self.add_child(Kind::Paragraph, self.next_nonspace);
                self.advance_next_nonspace();
                self.add_line();
            }
        }
    }

    fn continue_block(&mut self, id: usize) -> Continue {
        match self.nodes[id].kind {
            Kind::Document | Kind::List { .. } => Continue::Matched,
            Kind::BlockQuote => {
                if !self.indented && self.peek(self.next_nonspace) == Some(b'>') {
                    self.advance_next_nonspace();
                    self.advance_offset(1, false);
                    if self.peek(self.offset).is_some_and(scan::is_space_or_tab) {
                        self.advance_offset(1, true);
                    }
                    Continue::Matched
                } else {
                    Continue::Failed
                }
            }
            Kind::Item { data } => {
                if self.blank {
                    if self.nodes[id].children.is_empty() {
                        // 清單項目最多只能以一個空行開頭
                        return Continue::Failed;
                    }
                    self.advance_next_nonspace();
                } else if self.indent >= data.marker_offset + data.padding {
                    self.advance_offset(data.marker_offset + data.padding, true);
                } else {
                    return Continue::Failed;
                }
                Continue::Matched
            }
            Kind::Heading { .. } | Kind::ThematicBreak => Continue::Failed,
            Kind::CodeBlock { fence: Some(fence) } => {
                let rest = &self.line[self.next_nonspace..];
                if self.indent <= 3 && rest.as_bytes().first() == Some(&fence.ch) {
                    let n = rest.bytes().take_while(|&c| c == fence.ch).count();
                    if n >= fence.len && scan::is_blank(&rest[n..]) {
                        self.finalize(id, self.line_number);
                        return Continue::Consumed;
                    }
                }
                let mut i = fence.offset;
                while i > 0 && self.peek(self.offset).is_some_and(scan::is_space_or_tab) {
                    self.advance_offset(1, true);
                    i -= 1;
                }
                Continue::Matched
            }
            Kind::CodeBlock { fence: None } => {
                if self.indent >= CODE_INDENT {
                    self.advance_offset(CODE_INDENT, true);
                } else if self.blank {
                    self.advance_next_nonspace();
                } else {
                    return Continue::Failed;
                }
                Continue::Matched
            }
            Kind::Html { kind } => {
                if self.blank && (kind == 6 || kind == 7) {
                    Continue::Failed
                } else {
                    Continue::Matched
                }
            }
            Kind::Paragraph | Kind::Table { .. } => {
                if self.blank {
                    Continue::Failed
                } else {
                    Continue::Matched
                }
            }
            Kind::MathBlock { offset } => {
                let mut i = offset;
                while i > 0 && self.peek(self.offset).is_some_and(scan::is_space_or_tab) {
                    self.advance_offset(1, true);
                    i -= 1;
                }
                // 以 `$$` 結尾的行關閉區塊，`$$` 之前的內容仍屬於公式
                let rest = self.line[self.offset..].trim_end_matches([' ', '\t']);
                if let Some(content) = rest.strip_suffix("$$") {
                    if !scan::is_blank(content) {
                        self.nodes[id].lines.push(Line {
                            pad: 0,
                            text: content,
                        });
                    }
                    self.finalize(id, self.line_number);
                    return Continue::Consumed;
                }
                Continue::Matched
            }
            Kind::FootnoteDef => {
                if self.indent >= CODE_INDENT {
                    self.advance_offset(CODE_INDENT, true);
                } else if self.blank {
                    self.advance_next_nonspace();
                } else {
                    return Continue::Failed;
                }
                Continue::Matched
            }
        }
    }

    fn block_start(&mut self, container: usize) -> Start {
        let rest = &self.line[self.next_nonspace..];
        let first = rest.as_bytes().first().copied();
        let in_paragraph = matches!(self.nodes[container].kind, Kind::Paragraph);

        if !self.indented {
            // 引言
            if first == Some(b'>') {
                self.advance_next_nonspace();
                self.advance_offset(1, false);
                if self.peek(self.offset).is_some_and(scan::is_space_or_tab) {
                    self.advance_offset(1, true);
                }
                self.close_unmatched_blocks();
                self.add_child(Kind::BlockQuote, self.next_nonspace);
                return Start::Container;
            }

            // ATX 標題
            if let Some((level, content)) = atx_heading(rest) {
                self.advance_next_nonspace();
                self.close_unmatched_blocks();
                let id = self.add_child(Kind::Heading { level }, self.next_nonspace);
                self.nodes[id].content = Cow::Borrowed(content);
                self.advance_to_end();
                return Start::Leaf;
            }

            // fenced 程式碼
            if let Some(len) = code_fence(rest) {
                let fence = Fence {
                    ch: rest.as_bytes()[0],
                    len,
                    offset: self.indent,
                };
                self.close_unmatched_blocks();
                self.add_child(Kind::CodeBlock { fence: Some(fence) }, self.next_nonspace);
                self.advance_next_nonspace();
                self.advance_offset(len, false);
                return Start::Leaf;
            }

            // `$$` 數學區塊：同一行關閉（`$$ x $$`）或延續到以 `$$` 結尾的行
            if self.options.math
                && let Some(after) = rest.strip_prefix("$$")
            {
                match after.find("$$") {
                    Some(end) if scan::is_blank(&after[end + 2..]) => {
                        self.close_unmatched_blocks();
                        let id = self.add_child(Kind::MathBlock { offset: 0 }, self.next_nonspace);
                        self.nodes[id].content = Cow::Borrowed(after[..end].trim());
                        self.advance_to_end();
                        self.finalize(id, self.line_number);
                        return Start::Leaf;
                    }
                    // 同一行還有其他內容：當成段落中的行內公式
                    Some(_) => {}
                    None => {
                        let offset = self.indent;
                        self.close_unmatched_blocks();
                        self.add_child(Kind::MathBlock { offset }, self.next_nonspace);
                        self.advance_next_nonspace();
                        self.advance_offset(2, false);
                        return Start::Leaf;
                    }
                }
            }

            // HTML 區塊：類型 7 不能打斷段落（包含 lazy continuation 中的段落）
            let interrupts_paragraph = in_paragraph
                || (!self.all_closed
                    && !self.blank
                    && matches!(self.nodes[self.tip].kind, Kind::Paragraph));
            if let Some(kind) = scan::html_block_start(rest, interrupts_paragraph) {
                self.close_unmatched_blocks();
                self.add_child(Kind::Html { kind }, self.offset);
                return Start::Leaf;
            }

            // 腳註定義 `[^label]:`（不能打斷段落）
            if self.options.gfm
                && !in_paragraph
                && let Some(label) = footnote_def_label(rest)
            {
                self.advance_next_nonspace();
                self.close_unmatched_blocks();
                let id = self.add_child(Kind::FootnoteDef, self.next_nonspace);
                self.advance_offset(label.len() + 4, false);
                self.nodes[id].content = Cow::Borrowed(label);
                self.footnotes.insert(scan::normalize_label(label));
                return Start::Container;
            }

            // 表格：段落的最後一行是表頭，這一行是分隔列，且兩者欄數相同
            if self.options.gfm
                && in_paragraph
                && let Some(align) = table_delimiter_row(rest)
                && let Some(&header) = self.nodes[container].lines.last()
                && split_row(header.text).len() == align.len()
            {
                self.close_unmatched_blocks();
                let mut lines = std::mem::take(&mut self.nodes[container].lines);
                lines.pop();
                let table = if lines.is_empty() {
                    self.nodes[container].kind = Kind::Table { align };
                    container
                } else {
                    // 表頭之前的行仍是段落
                    self.nodes[container].lines = lines;
                    self.finalize(container, self.line_number - 2);
                    let id = self.add_child(Kind::Table { align }, 0);
                    self.nodes[id].start = Span {
                        line: self.line_number - 1,
                        col: 1,
                    };
                    id
                };
                self.nodes[table].lines = vec![header];
                self.advance_to_end();
                return Start::Leaf;
            }

            // setext 標題
            if in_paragraph && let Some(level) = setext_underline(rest) {
                self.close_unmatched_blocks();
                let consumed = self.extract_link_defs(container);
                self.nodes[container].lines.drain(..consumed);
                if !self.nodes[container].lines.is_empty() {
                    let content = self.join_lines(&self.nodes[container].lines, false);
                    let node = &mut self.nodes[container];
                    node.kind = Kind::Heading { level };
                    node.content = content;
                    node.lines.clear();
                    self.advance_to_end();
                    return Start::Leaf;
                }
            }

            // 分隔線
            if is_thematic_break(rest) {
                self.close_unmatched_blocks();
                self.add_child(Kind::ThematicBreak, self.next_nonspace);
                self.advance_to_end();
                return Start::Leaf;
            }
        }

        // 清單項目
        if (!self.indented || matches!(self.nodes[container].kind, Kind::List { .. }))
            && let Some(data) = self.parse_list_marker(in_paragraph)
        {
            self.close_unmatched_blocks();
            let continues_list = matches!(
                self.nodes[self.tip].kind,
                Kind::List { data: ref list, .. } if list.ty == data.ty
            );
            if !continues_list {
                self.add_child(Kind::List { data, tight: true }, self.next_nonspace);
            }
            self.add_child(Kind::Item { data }, self.next_nonspace);
            return Start::Container;
        }

        // 縮排程式碼：不能打斷段落或表格
        if self.indented
            && !matches!(
                self.nodes[self.tip].kind,
                Kind::Paragraph | Kind::Table { .. }
            )
            && !self.blank
        {
            self.advance_offset(CODE_INDENT, true);
            self.close_unmatched_blocks();
            self.add_child(Kind::CodeBlock { fence: None }, self.offset);
            return Start::Leaf;
        }

        Start::None
    }

    fn parse_list_marker(&mut self, in_paragraph: bool) -> Option<ListData> {
        if self.indent >= CODE_INDENT {
            return None;
        }
        let rest = &self.line[self.next_nonspace..];
        let b = rest.as_bytes();
        let (ty, start, marker_len) = match *b.first()? {
            c @ (b'*' | b'+' | b'-') => (ListType::Bullet(c), 1, 1),
            _ => {
                let digits = b.iter().take_while(|c| c.is_ascii_digit()).count();
                let delim = *b.get(digits)?;
                if !(1..=9).contains(&digits) || !matches!(delim, b'.' | b')') {
                    return None;
                }
                let start: u32 = rest[..digits].parse().ok()?;
                // 打斷段落的有序清單必須從 1 開始
                if in_paragraph && start != 1 {
                    return None;
                }
                (ListType::Ordered(delim), start, digits + 1)
            }
        };
        if !b.get(marker_len).is_none_or(|&c| scan::is_space_or_tab(c)) {
            return None;
        }
        // 打斷段落的清單項目不能是空的
        if in_paragraph && scan::is_blank(&rest[marker_len..]) {
            return None;
        }

        let marker_offset = self.indent;
        self.advance_next_nonspace();
        self.advance_offset(marker_len, true);
        let spaces_start_col = self.column;
        let spaces_start_offset = self.offset;
        loop {
            self.advance_offset(1, true);
            let next = self.peek(self.offset);
            if !(self.column - spaces_start_col < 5 && next.is_some_and(scan::is_space_or_tab)) {
                break;
            }
        }
        let blank_item = self.offset >= self.line.len();
        let spaces_after = self.column - spaces_start_col;
        let padding = if !(1..5).contains(&spaces_after) || blank_item {
            // 標記後有 5 格以上空白時，內容視為縮排程式碼，padding 只算一格
            self.column = spaces_start_col;
            self.offset = spaces_start_offset;
            self.partially_consumed_tab = false;
            if self.peek(self.offset).is_some_and(scan::is_space_or_tab) {
                self.advance_offset(1, true);
            }
            marker_len + 1
        } else {
            marker_len + spaces_after
        };
        Some(ListData {
            ty,
            start,
            marker_offset,
            padding,
        })
    }

    // ---- 關閉區塊 ----

    fn finalize(&mut self, id: usize, line_number: u32) {
        let parent = self.nodes[id].parent;
        self.nodes[id].open = false;
        self.nodes[id].end_line = line_number;

        match self.nodes[id].kind {
            Kind::Paragraph => {
                let consumed = self.extract_link_defs(id);
                self.nodes[id].lines.drain(..consumed);
                if self.nodes[id].lines.is_empty() {
                    self.nodes[parent].children.retain(|&c| c != id);
                } else {
                    self.nodes[id].content = self.join_lines(&self.nodes[id].lines, false);
                }
            }
            Kind::CodeBlock { fence } => {
                let mut lines = std::mem::take(&mut self.nodes[id].lines);
                if fence.is_some() {
                    // 第一行是 info string
                    let info = if lines.is_empty() {
                        ""
                    } else {
                        lines.remove(0).text
                    };
                    self.nodes[id].lang = info.split_whitespace().next().map(scan::unescape);
                } else {
                    while lines
                        .last()
                        .is_some_and(|l| l.text.bytes().all(|c| c == b' '))
                    {
                        lines.pop();
                    }
                }
                self.nodes[id].content = self.join_lines(&lines, true);
            }
            Kind::Html { .. } => {
                let lines = std::mem::take(&mut self.nodes[id].lines);
                self.nodes[id].content = self.join_lines(&lines, false);
            }
            // 單行的數學區塊在建立時就已設定內容
            Kind::MathBlock { .. } if !self.nodes[id].lines.is_empty() => {
                let lines = std::mem::take(&mut self.nodes[id].lines);
                self.nodes[id].content = match self.join_lines(&lines, false) {
                    Cow::Borrowed(s) => Cow::Borrowed(s.trim()),
                    Cow::Owned(s) => Cow::Owned(s.trim().to_string()),
                };
            }
            // 清單與項目的結束行取最後一個子區塊，尾端的空行不算在內，
            // 這樣「兩區塊間是否隔著空行」只需比較行號。
            Kind::Item { .. } | Kind::List { .. } => {
                let node = &self.nodes[id];
                let end = node
                    .children
                    .last()
                    .map_or(node.start.line, |&c| self.nodes[c].end_line);
                let is_list = matches!(node.kind, Kind::List { .. });
                let tight = is_list && self.is_tight(id);
                let node = &mut self.nodes[id];
                node.end_line = end;
                if let Kind::List { tight: t, .. } = &mut node.kind {
                    *t = tight;
                }
            }
            _ => {}
        }
        self.tip = parent;
    }

    /// 清單中任兩個相鄰項目之間、或項目內任兩個相鄰區塊之間有空行時，清單為 loose。
    fn is_tight(&self, list: usize) -> bool {
        let separated = |a: usize, b: usize| self.nodes[a].end_line + 1 != self.nodes[b].start.line;
        let items = &self.nodes[list].children;
        for (i, &item) in items.iter().enumerate() {
            if let Some(&next) = items.get(i + 1)
                && separated(item, next)
            {
                return false;
            }
            let children = &self.nodes[item].children;
            if children.windows(2).any(|w| separated(w[0], w[1])) {
                return false;
            }
        }
        true
    }

    /// 從段落開頭解析連結參照定義並登記，回傳被定義佔用的行數。
    fn extract_link_defs(&mut self, id: usize) -> usize {
        let content = self.join_lines(&self.nodes[id].lines, false);
        let (consumed, defs) = match &content {
            Cow::Borrowed(s) => parse_link_defs(s),
            Cow::Owned(s) => {
                let (n, defs) = parse_link_defs(s);
                (
                    n,
                    defs.into_iter().map(|(k, d)| (k, d.into_owned())).collect(),
                )
            }
        };
        for (label, def) in defs {
            self.link_defs.entry(label).or_insert(def);
        }
        let newlines = content[..consumed].matches('\n').count();
        if consumed == content.len() && consumed > 0 {
            newlines + usize::from(!content.ends_with('\n'))
        } else {
            newlines
        }
    }

    /// 把多行接成一個字串。若這些行在原始輸入中本來就是連續的，直接借用原始輸入。
    fn join_lines(&self, lines: &[Line<'a>], trailing_newline: bool) -> Cow<'a, str> {
        let (Some(first), Some(last)) = (lines.first(), lines.last()) else {
            return Cow::Borrowed("");
        };
        let base = self.input.as_ptr() as usize;
        let start_of = |l: &Line| l.text.as_ptr() as usize - base;
        let end_of = |l: &Line| start_of(l) + l.text.len();
        let bytes = self.input.as_bytes();

        let contiguous = lines.iter().all(|l| l.pad == 0)
            && lines
                .windows(2)
                .all(|w| start_of(&w[1]) == end_of(&w[0]) + 1 && bytes[end_of(&w[0])] == b'\n');
        if contiguous {
            let end = end_of(last);
            if !trailing_newline {
                return Cow::Borrowed(&self.input[start_of(first)..end]);
            }
            if bytes.get(end) == Some(&b'\n') {
                return Cow::Borrowed(&self.input[start_of(first)..end + 1]);
            }
        }

        let mut out = String::new();
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out.extend(std::iter::repeat_n(' ', line.pad));
            out.push_str(line.text);
        }
        if trailing_newline {
            out.push('\n');
        }
        Cow::Owned(out)
    }

    // ---- 輸出 ----

    fn finish(mut self) -> Document<'a> {
        while self.tip != ROOT {
            self.finalize(self.tip, self.line_number);
        }
        self.finalize(ROOT, self.line_number);

        let mut nodes: Vec<Option<Node<'a>>> = self.nodes.into_iter().map(Some).collect();
        let children = nodes[ROOT].take().unwrap().children;
        let ctx = Ctx {
            links: &self.link_defs,
            footnotes: &self.footnotes,
            options: self.options,
        };
        let blocks = build_blocks(&mut nodes, &children, &ctx);
        Document {
            blocks,
            link_defs: self.link_defs,
        }
    }
}

impl<'a> Node<'a> {
    fn new(kind: Kind, parent: usize, start: Span) -> Self {
        Node {
            kind,
            parent,
            children: Vec::new(),
            open: true,
            start,
            end_line: start.line,
            lines: Vec::new(),
            content: Cow::Borrowed(""),
            lang: None,
        }
    }
}

fn build_blocks<'a>(
    nodes: &mut [Option<Node<'a>>],
    ids: &[usize],
    ctx: &Ctx<'_, 'a>,
) -> Vec<Block<'a>> {
    ids.iter().map(|&id| build_block(nodes, id, ctx)).collect()
}

fn build_block<'a>(nodes: &mut [Option<Node<'a>>], id: usize, ctx: &Ctx<'_, 'a>) -> Block<'a> {
    let node = nodes[id].take().expect("每個節點只會被建構一次");
    let kind = match node.kind {
        Kind::Paragraph => BlockKind::Paragraph(inline::parse(node.content, ctx)),
        Kind::Heading { level } => BlockKind::Heading {
            level,
            content: inline::parse(node.content, ctx),
        },
        Kind::BlockQuote => BlockKind::BlockQuote(build_blocks(nodes, &node.children, ctx)),
        Kind::List { data, tight } => BlockKind::List {
            ordered: matches!(data.ty, ListType::Ordered(_)).then_some(data.start),
            tight,
            items: node
                .children
                .iter()
                .map(|&item| build_item(nodes, item, ctx))
                .collect(),
        },
        // ```math 視為數學區塊
        Kind::CodeBlock { .. } if ctx.options.math && node.lang.as_deref() == Some("math") => {
            BlockKind::MathBlock(match node.content {
                Cow::Borrowed(s) => Cow::Borrowed(s.trim_end()),
                Cow::Owned(s) => Cow::Owned(s.trim_end().to_string()),
            })
        }
        Kind::MathBlock { .. } => BlockKind::MathBlock(node.content),
        Kind::CodeBlock { .. } => BlockKind::CodeBlock {
            lang: node.lang,
            code: node.content,
        },
        Kind::Html { .. } if ctx.options.page_break && is_page_break(&node.content) => {
            BlockKind::PageBreak
        }
        Kind::Html { .. } => BlockKind::Html(node.content),
        Kind::ThematicBreak => BlockKind::ThematicBreak,
        Kind::Table { align } => {
            let columns = align.len();
            let mut row = |line: &Line<'a>| -> Vec<Cell<'a>> {
                let mut cells: Vec<Cell<'a>> = split_row(line.text)
                    .into_iter()
                    .take(columns)
                    .map(|c| inline::parse(c, ctx))
                    .collect();
                cells.resize_with(columns, Vec::new);
                cells
            };
            let head = row(&node.lines[0]);
            let rows = node.lines[2..].iter().map(&mut row).collect();
            BlockKind::Table { align, head, rows }
        }
        Kind::FootnoteDef => BlockKind::FootnoteDef {
            label: node.content,
            blocks: build_blocks(nodes, &node.children, ctx),
        },
        Kind::Item { .. } | Kind::Document => unreachable!("清單項目由清單建構"),
    };
    Block {
        kind,
        span: node.start,
    }
}

fn build_item<'a>(nodes: &mut [Option<Node<'a>>], id: usize, ctx: &Ctx<'_, 'a>) -> ListItem<'a> {
    let item = nodes[id].take().unwrap();
    let mut task = None;
    // 任務清單：第一個子區塊是以 `[ ]`、`[x]` 開頭的段落
    if ctx.options.gfm
        && let Some(&first) = item.children.first()
        && let Some(para) = nodes[first].as_mut()
        && matches!(para.kind, Kind::Paragraph)
        && let Some((checked, skip)) = task_marker(&para.content)
    {
        task = Some(checked);
        para.content = match std::mem::take(&mut para.content) {
            Cow::Borrowed(s) => Cow::Borrowed(&s[skip..]),
            Cow::Owned(s) => Cow::Owned(s[skip..].to_string()),
        };
    }
    ListItem {
        blocks: build_blocks(nodes, &item.children, ctx),
        span: item.start,
        task,
    }
}

/// 內容只有 `<!-- pagebreak -->` 的 HTML 區塊（不分大小寫，註解內可有空白）。
fn is_page_break(html: &str) -> bool {
    html.trim()
        .strip_prefix("<!--")
        .and_then(|s| s.strip_suffix("-->"))
        .is_some_and(|s| s.trim().eq_ignore_ascii_case("pagebreak"))
}

/// `[ ] `、`[x] `：回傳（是否勾選, 標記與其後空白的長度）。
fn task_marker(s: &str) -> Option<(bool, usize)> {
    let b = s.as_bytes();
    if b.len() < 4 || b[0] != b'[' || b[2] != b']' || !scan::is_space_or_tab(b[3]) {
        return None;
    }
    let checked = match b[1] {
        b' ' | b'\t' => false,
        b'x' | b'X' => true,
        _ => return None,
    };
    let skip = 3 + b[3..]
        .iter()
        .take_while(|&&c| scan::is_space_or_tab(c))
        .count();
    Some((checked, skip))
}

/// `[^label]:` 的標籤。標籤不能含空白或方括號。
fn footnote_def_label(rest: &str) -> Option<&str> {
    let inner = rest.strip_prefix("[^")?;
    let end = inner.find(']')?;
    let label = &inner[..end];
    let valid = !label.is_empty() && !label.contains(|c: char| c.is_whitespace() || c == '[');
    (valid && inner[end + 1..].starts_with(':')).then_some(label)
}

/// 表格分隔列，例如 `| :-- | :-: | --: |`。必須含有 `|`。
fn table_delimiter_row(rest: &str) -> Option<Vec<Align>> {
    if !rest.contains('|') {
        return None;
    }
    split_row(rest)
        .iter()
        .map(|cell| {
            let left = cell.starts_with(':');
            let right = cell.len() > 1 && cell.ends_with(':');
            let dashes = &cell[usize::from(left)..cell.len() - usize::from(right)];
            if dashes.is_empty() || !dashes.bytes().all(|c| c == b'-') {
                return None;
            }
            Some(match (left, right) {
                (true, true) => Align::Center,
                (true, false) => Align::Left,
                (false, true) => Align::Right,
                (false, false) => Align::None,
            })
        })
        .collect()
}

/// 把表格的一列拆成儲存格：去掉首尾的 `|`，以未跳脫的 `|` 分隔，`\|` 還原為 `|`。
fn split_row(line: &str) -> Vec<Cow<'_, str>> {
    let s = line.trim_matches([' ', '\t']);
    let s = s.strip_prefix('|').unwrap_or(s);
    let b = s.as_bytes();
    let mut cells = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            b'|' => {
                cells.push(&s[start..i]);
                i += 1;
                start = i;
            }
            _ => i += 1,
        }
    }
    // 結尾沒有 `|` 時，最後一段也是儲存格
    if start < b.len() || cells.is_empty() {
        cells.push(&s[start.min(b.len())..]);
    }
    cells
        .into_iter()
        .map(|cell| {
            let cell = cell.trim_matches([' ', '\t']);
            if cell.contains("\\|") {
                Cow::Owned(cell.replace("\\|", "|"))
            } else {
                Cow::Borrowed(cell)
            }
        })
        .collect()
}

fn parse_link_defs(s: &str) -> (usize, Vec<(String, LinkDef<'_>)>) {
    let mut pos = 0;
    let mut defs = Vec::new();
    while s[pos..].starts_with('[')
        && let Some((n, label, def)) = scan::link_reference_def(&s[pos..])
    {
        defs.push((label, def));
        pos += n;
    }
    (pos, defs)
}

/// ATX 標題：1–6 個 `#` 之後接空白或行尾。回傳（層級, 去除結尾 `#` 序列的內容）。
fn atx_heading(rest: &str) -> Option<(u8, &str)> {
    let level = rest.bytes().take_while(|&c| c == b'#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let after = &rest[level..];
    if !after.is_empty() && !after.starts_with([' ', '\t']) {
        return None;
    }
    let mut content = after.trim_matches([' ', '\t']);
    let without_closing = content.trim_end_matches('#');
    if without_closing.is_empty() || without_closing.ends_with([' ', '\t']) {
        content = without_closing.trim_end_matches([' ', '\t']);
    }
    Some((level as u8, content))
}

/// 開頭 fence：至少 3 個反引號（info 中不能有反引號）或波浪號。回傳 fence 長度。
fn code_fence(rest: &str) -> Option<usize> {
    let ch = *rest.as_bytes().first()?;
    if ch != b'`' && ch != b'~' {
        return None;
    }
    let len = rest.bytes().take_while(|&c| c == ch).count();
    if len < 3 || (ch == b'`' && rest[len..].contains('`')) {
        return None;
    }
    Some(len)
}

fn setext_underline(rest: &str) -> Option<u8> {
    let ch = *rest.as_bytes().first()?;
    let level = match ch {
        b'=' => 1,
        b'-' => 2,
        _ => return None,
    };
    let n = rest.bytes().take_while(|&c| c == ch).count();
    scan::is_blank(&rest[n..]).then_some(level)
}

fn is_thematic_break(rest: &str) -> bool {
    let Some(&ch) = rest.as_bytes().first() else {
        return false;
    };
    if !matches!(ch, b'*' | b'-' | b'_') {
        return false;
    }
    let mut count = 0;
    for c in rest.bytes() {
        if c == ch {
            count += 1;
        } else if !scan::is_space_or_tab(c) {
            return false;
        }
    }
    count >= 3
}
