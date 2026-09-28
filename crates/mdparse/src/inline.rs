//! Inline phase: parse the text of a paragraph or heading into a sequence of [`Inline`] nodes.
//!
//! Follows the algorithm from the CommonMark spec appendix: while scanning, runs of `*` and `_` and
//! the brackets `[` and `![` are recorded on the delimiter and bracket stacks; a `]` tries to form a
//! link, and `process_emphasis` finally pairs up emphasis.
//!
//! During parsing, nodes live in an arena and siblings form a doubly linked list, so moving a range of
//! siblings into a new emphasis or link node never invalidates existing node indices.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use crate::ast::{Inline, LinkDef};
use crate::{Options, scan};

/// Document-level information needed for inline parsing.
pub struct Ctx<'c, 'a> {
    pub links: &'c HashMap<String, LinkDef<'a>>,
    /// Labels of defined footnotes (normalized).
    pub footnotes: &'c HashSet<String>,
    pub options: Options,
}

/// Parse one piece of content whose first line is source line `line`. Borrowed content yields
/// borrowed nodes; otherwise the result is converted to owned.
pub fn parse<'a>(content: Cow<'a, str>, ctx: &Ctx<'_, 'a>, line: u32) -> Vec<Inline<'a>> {
    match content {
        Cow::Borrowed(s) => parse_str(s, ctx, line),
        Cow::Owned(s) => parse_str(&s, ctx, line)
            .into_iter()
            .map(Inline::into_owned)
            .collect(),
    }
}

/// Content lines map one-to-one to source lines, so the line of any position is `line` plus the
/// number of newlines before it.
pub fn parse_str<'s, 'a: 's>(s: &'s str, ctx: &Ctx<'_, 'a>, line: u32) -> Vec<Inline<'s>> {
    let trimmed = s.trim_start_matches([' ', '\t', '\n']);
    let line = line + s[..s.len() - trimmed.len()].matches('\n').count() as u32;
    let s = trimmed.trim_end_matches([' ', '\t', '\n']);
    let mut p = Parser {
        s,
        pos: 0,
        line,
        refs: ctx.links,
        footnotes: ctx.footnotes,
        options: ctx.options,
        nodes: vec![Node::new(Kind::Root)],
        delims: Vec::new(),
        top: None,
        brackets: Vec::new(),
    };
    while p.pos < s.len() {
        p.parse_inline();
    }
    p.process_emphasis(None);
    p.build(ROOT)
}

const ROOT: usize = 0;

#[derive(Debug)]
enum Kind<'s> {
    Root,
    Text(Cow<'s, str>),
    Code(Cow<'s, str>),
    Html(&'s str),
    SoftBreak,
    HardBreak,
    Emph,
    Strong,
    Strike,
    FootnoteRef(&'s str),
    Math {
        tex: &'s str,
        display: bool,
        line: u32,
    },
    Link {
        url: Cow<'s, str>,
        title: Option<Cow<'s, str>>,
    },
    Image {
        url: Cow<'s, str>,
        title: Option<Cow<'s, str>>,
        line: u32,
    },
}

#[derive(Debug)]
struct Node<'s> {
    kind: Kind<'s>,
    parent: Option<usize>,
    prev: Option<usize>,
    next: Option<usize>,
    first: Option<usize>,
    last: Option<usize>,
}

impl<'s> Node<'s> {
    fn new(kind: Kind<'s>) -> Self {
        Node {
            kind,
            parent: None,
            prev: None,
            next: None,
            first: None,
            last: None,
        }
    }
}

/// A delimiter stack entry: a run of `*`, `_` or `~`.
#[derive(Debug)]
struct Delim {
    ch: u8,
    /// Number of characters not yet used.
    count: usize,
    /// Original length (for the "multiple of 3" rule).
    orig: usize,
    node: usize,
    prev: Option<usize>,
    next: Option<usize>,
    can_open: bool,
    can_close: bool,
}

#[derive(Debug)]
struct Bracket {
    node: usize,
    /// Top of the delimiter stack when this bracket was pushed.
    prev_delim: Option<usize>,
    /// Position of `[` in the source.
    index: usize,
    image: bool,
    active: bool,
    /// Whether another `[` follows (if so, a shortcut reference need not be tried).
    bracket_after: bool,
}

struct Parser<'s, 'm> {
    s: &'s str,
    pos: usize,
    /// Source line of the start of `s`.
    line: u32,
    refs: &'m HashMap<String, LinkDef<'s>>,
    footnotes: &'m HashSet<String>,
    options: Options,
    nodes: Vec<Node<'s>>,
    delims: Vec<Delim>,
    top: Option<usize>,
    brackets: Vec<Bracket>,
}

impl<'s> Parser<'s, '_> {
    // ---- tree operations ----

    fn add(&mut self, parent: usize, kind: Kind<'s>) -> usize {
        let id = self.nodes.len();
        self.nodes.push(Node::new(kind));
        self.append_child(parent, id);
        id
    }

    fn text(&mut self, text: impl Into<Cow<'s, str>>) -> usize {
        self.add(ROOT, Kind::Text(text.into()))
    }

    fn append_child(&mut self, parent: usize, child: usize) {
        let last = self.nodes[parent].last;
        self.nodes[child].parent = Some(parent);
        self.nodes[child].prev = last;
        self.nodes[child].next = None;
        match last {
            Some(last) => self.nodes[last].next = Some(child),
            None => self.nodes[parent].first = Some(child),
        }
        self.nodes[parent].last = Some(child);
    }

    fn insert_after(&mut self, node: usize, sibling: usize) {
        let parent = self.nodes[node].parent;
        let next = self.nodes[node].next;
        self.nodes[sibling].parent = parent;
        self.nodes[sibling].prev = Some(node);
        self.nodes[sibling].next = next;
        self.nodes[node].next = Some(sibling);
        match next {
            Some(next) => self.nodes[next].prev = Some(sibling),
            None => {
                if let Some(parent) = parent {
                    self.nodes[parent].last = Some(sibling);
                }
            }
        }
    }

    fn unlink(&mut self, node: usize) {
        let Node {
            parent, prev, next, ..
        } = self.nodes[node];
        match prev {
            Some(prev) => self.nodes[prev].next = next,
            None => {
                if let Some(parent) = parent {
                    self.nodes[parent].first = next;
                }
            }
        }
        match next {
            Some(next) => self.nodes[next].prev = prev,
            None => {
                if let Some(parent) = parent {
                    self.nodes[parent].last = prev;
                }
            }
        }
        let n = &mut self.nodes[node];
        n.parent = None;
        n.prev = None;
        n.next = None;
    }

    /// Move the siblings after `after` (up to, but excluding, `until`) into `container`.
    fn move_siblings(&mut self, after: usize, until: Option<usize>, container: usize) {
        let mut cur = self.nodes[after].next;
        while let Some(node) = cur {
            if Some(node) == until {
                break;
            }
            cur = self.nodes[node].next;
            self.unlink(node);
            self.append_child(container, node);
        }
    }

    // ---- scanning ----

    fn line_at(&self, pos: usize) -> u32 {
        self.line + self.s[..pos].matches('\n').count() as u32
    }

    fn peek(&self) -> Option<u8> {
        self.s.as_bytes().get(self.pos).copied()
    }

    fn parse_inline(&mut self) {
        match self.peek().unwrap() {
            b'\n' => self.newline(),
            b'\\' => self.backslash(),
            b'`' => self.backticks(),
            b'*' | b'_' => self.delim_run(),
            b'~' if self.options.gfm => self.delim_run(),
            b'$' if self.options.math && self.math() => {}
            b'[' if self.options.gfm && self.footnote_ref() => {}
            b'[' => {
                let node = self.text(&self.s[self.pos..self.pos + 1]);
                self.push_bracket(node, self.pos, false);
                self.pos += 1;
            }
            b'!' if self.s.as_bytes().get(self.pos + 1) == Some(&b'[') => {
                let node = self.text(&self.s[self.pos..self.pos + 2]);
                self.push_bracket(node, self.pos + 1, true);
                self.pos += 2;
            }
            b']' => self.close_bracket(),
            b'<' => self.angle(),
            b'&' => match scan::entity(&self.s[self.pos..]) {
                Some((n, decoded)) => {
                    self.text(decoded);
                    self.pos += n;
                }
                None => self.single_char(),
            },
            _ => self.string(),
        }
    }

    fn single_char(&mut self) {
        let len = self.s[self.pos..].chars().next().unwrap().len_utf8();
        self.text(&self.s[self.pos..self.pos + len]);
        self.pos += len;
    }

    /// Plain text: read up to the next character that may be special.
    fn string(&mut self) {
        let start = self.pos;
        let rest = &self.s.as_bytes()[start..];
        let n = rest
            .iter()
            .skip(1)
            .position(|c| b"\n\\`*_~[]!<&$".contains(c))
            .map_or(rest.len(), |i| i + 1);
        self.pos += n;
        self.text(&self.s[start..self.pos]);
    }

    fn newline(&mut self) {
        self.pos += 1;
        let mut hard = false;
        if let Some(last) = self.nodes[ROOT].last
            && let Kind::Text(text) = &mut self.nodes[last].kind
            && text.ends_with(' ')
        {
            hard = text.ends_with("  ");
            *text = match std::mem::take(text) {
                Cow::Borrowed(s) => Cow::Borrowed(s.trim_end_matches(' ')),
                Cow::Owned(s) => Cow::Owned(s.trim_end_matches(' ').to_string()),
            };
        }
        self.add(
            ROOT,
            if hard {
                Kind::HardBreak
            } else {
                Kind::SoftBreak
            },
        );
        self.skip_spaces();
    }

    fn skip_spaces(&mut self) {
        while self.peek() == Some(b' ') {
            self.pos += 1;
        }
    }

    fn backslash(&mut self) {
        self.pos += 1;
        match self.peek() {
            Some(b'\n') => {
                self.pos += 1;
                self.add(ROOT, Kind::HardBreak);
                self.skip_spaces();
            }
            Some(c) if scan::is_ascii_punct(c) => {
                self.text(&self.s[self.pos..self.pos + 1]);
                self.pos += 1;
            }
            _ => {
                self.text(&self.s[self.pos - 1..self.pos]);
            }
        }
    }

    fn backticks(&mut self) {
        let b = self.s.as_bytes();
        let start = self.pos;
        let run = b[start..].iter().take_while(|&&c| c == b'`').count();
        self.pos += run;
        match find_closing_backticks(b, self.pos, run) {
            Some(close) => {
                let code = code_span_content(&self.s[self.pos..close]);
                self.add(ROOT, Kind::Code(code));
                self.pos = close + run;
            }
            // Without a matching closing run, the whole backtick run is plain text
            None => {
                self.text(&self.s[start..self.pos]);
            }
        }
    }

    fn angle(&mut self) {
        let rest = &self.s[self.pos..];
        if let Some((n, addr)) = scan::autolink_email(rest) {
            let link = self.add(
                ROOT,
                Kind::Link {
                    url: Cow::Owned(format!("mailto:{addr}")),
                    title: None,
                },
            );
            self.add(link, Kind::Text(Cow::Borrowed(addr)));
            self.pos += n;
        } else if let Some((n, uri)) = scan::autolink_uri(rest) {
            let link = self.add(
                ROOT,
                Kind::Link {
                    url: Cow::Borrowed(uri),
                    title: None,
                },
            );
            self.add(link, Kind::Text(Cow::Borrowed(uri)));
            self.pos += n;
        } else if let Some(n) = scan::inline_html(rest) {
            self.add(ROOT, Kind::Html(&rest[..n]));
            self.pos += n;
        } else {
            self.single_char();
        }
    }

    // ---- emphasis ----

    /// Math `$...$` or `$$...$$`, keeping the LaTeX source.
    ///
    /// Inline math follows pandoc's rules so that prices are not mistaken for formulas: the opening `$`
    /// must not be followed by whitespace, and the closing `$` must not follow whitespace or precede a digit.
    fn math(&mut self) -> bool {
        let b = self.s.as_bytes();
        let start = self.pos;
        let display = b.get(start + 1) == Some(&b'$');
        let content_start = start + if display { 2 } else { 1 };
        if !display && b.get(content_start).is_none_or(|c| c.is_ascii_whitespace()) {
            return false;
        }
        let mut i = content_start;
        let close = loop {
            match b.get(i) {
                None => return false,
                Some(b'\\') => i += 2,
                Some(b'$') if display => {
                    if b.get(i + 1) == Some(&b'$') {
                        break i;
                    }
                    i += 1;
                }
                Some(b'$') => {
                    let valid = !b[i - 1].is_ascii_whitespace()
                        && !b.get(i + 1).is_some_and(u8::is_ascii_digit);
                    if valid {
                        break i;
                    }
                    i += 1;
                }
                Some(_) => i += 1,
            }
        };
        let tex = self.s[content_start..close].trim_matches([' ', '\t', '\n']);
        if tex.is_empty() {
            return false;
        }
        let line = self.line_at(start);
        self.add(ROOT, Kind::Math { tex, display, line });
        self.pos = close + if display { 2 } else { 1 };
        true
    }

    /// `[^label]`: only valid when a matching footnote definition exists.
    fn footnote_ref(&mut self) -> bool {
        let Some(inner) = self.s[self.pos..].strip_prefix("[^") else {
            return false;
        };
        let Some(end) = inner.find(']') else {
            return false;
        };
        let label = &inner[..end];
        if label.is_empty()
            || label.contains(|c: char| c.is_whitespace() || c == '[')
            || !self.footnotes.contains(&scan::normalize_label(label))
        {
            return false;
        }
        self.add(ROOT, Kind::FootnoteRef(label));
        self.pos += end + 3;
        true
    }

    fn delim_run(&mut self) {
        let ch = self.peek().unwrap();
        let start = self.pos;
        let count = self.s.as_bytes()[start..]
            .iter()
            .take_while(|&&c| c == ch)
            .count();
        let (can_open, can_close) = self.flanking(start, count, ch);
        self.pos += count;
        let node = self.text(&self.s[start..self.pos]);
        // Strikethrough accepts only one or two `~`
        let usable = ch != b'~' || count <= 2;
        if usable && (can_open || can_close) {
            let id = self.delims.len();
            self.delims.push(Delim {
                ch,
                count,
                orig: count,
                node,
                prev: self.top,
                next: None,
                can_open,
                can_close,
            });
            if let Some(top) = self.top {
                self.delims[top].next = Some(id);
            }
            self.top = Some(id);
        }
    }

    /// Apply the left-/right-flanking rules to decide whether this delimiter run can open or close emphasis.
    fn flanking(&self, start: usize, count: usize, ch: u8) -> (bool, bool) {
        let before = self.s[..start].chars().next_back().unwrap_or('\n');
        let after = self.s[start + count..].chars().next().unwrap_or('\n');
        let before_ws = scan::is_unicode_whitespace(before);
        let before_punct = scan::is_unicode_punct(before);
        let after_ws = scan::is_unicode_whitespace(after);
        let after_punct = scan::is_unicode_punct(after);

        // CJK-friendly mode: a CJK character on the outer side counts like whitespace or punctuation
        let cjk = self.options.cjk_emphasis;
        let before_cjk = cjk && scan::is_cjk(before);
        let after_cjk = cjk && scan::is_cjk(after);

        let left = !after_ws && (!after_punct || before_ws || before_punct || before_cjk);
        let right = !before_ws && (!before_punct || after_ws || after_punct || after_cjk);
        if ch == b'_' {
            (
                left && (!right || before_punct),
                right && (!left || after_punct),
            )
        } else {
            (left, right)
        }
    }

    fn remove_delim(&mut self, d: usize) {
        let Delim { prev, next, .. } = self.delims[d];
        if let Some(prev) = prev {
            self.delims[prev].next = next;
        }
        match next {
            Some(next) => self.delims[next].prev = prev,
            None => self.top = prev,
        }
    }

    fn process_emphasis(&mut self, stack_bottom: Option<usize>) {
        // Search lower bounds kept per character, per "can open", and per "original length mod 3"
        let mut openers_bottom = [[stack_bottom; 6]; 3];

        let mut closer = self.top;
        while let Some(c) = closer
            && self.delims[c].prev != stack_bottom
        {
            closer = self.delims[c].prev;
        }

        while let Some(c) = closer {
            if !self.delims[c].can_close {
                closer = self.delims[c].next;
                continue;
            }
            let ch = self.delims[c].ch;
            let bottom_index = usize::from(self.delims[c].can_open) * 3 + self.delims[c].orig % 3;
            let row = match ch {
                b'*' => 0,
                b'_' => 1,
                _ => 2,
            };
            let bottom = &mut openers_bottom[row][bottom_index];

            let mut opener = self.delims[c].prev;
            let mut found = None;
            while let Some(o) = opener
                && opener != stack_bottom
                && opener != *bottom
            {
                let (od, cd) = (&self.delims[o], &self.delims[c]);
                let compatible = if ch == b'~' {
                    // Strikethrough opener and closer must have the same length
                    od.count == cd.count
                } else {
                    let odd_match = (cd.can_open || od.can_close)
                        && cd.orig % 3 != 0
                        && (od.orig + cd.orig) % 3 == 0;
                    !odd_match
                };
                if od.ch == ch && od.can_open && compatible {
                    found = Some(o);
                    break;
                }
                opener = od.prev;
            }

            let Some(o) = found else {
                *bottom = self.delims[c].prev;
                closer = self.delims[c].next;
                if !self.delims[c].can_open {
                    self.remove_delim(c);
                }
                continue;
            };

            let used = if ch == b'~' {
                self.delims[c].count
            } else if self.delims[c].count >= 2 && self.delims[o].count >= 2 {
                2
            } else {
                1
            };
            let (o_node, c_node) = (self.delims[o].node, self.delims[c].node);
            self.delims[o].count -= used;
            self.delims[c].count -= used;
            for node in [o_node, c_node] {
                if let Kind::Text(Cow::Borrowed(text)) = &mut self.nodes[node].kind {
                    *text = &text[..text.len() - used];
                }
            }

            let kind = match (ch, used) {
                (b'~', _) => Kind::Strike,
                (_, 2) => Kind::Strong,
                _ => Kind::Emph,
            };
            let emph = self.nodes.len();
            self.nodes.push(Node::new(kind));
            self.move_siblings(o_node, Some(c_node), emph);
            self.insert_after(o_node, emph);

            // Delimiters in between become inactive
            if self.delims[c].prev != Some(o) {
                self.delims[c].prev = Some(o);
                self.delims[o].next = Some(c);
            }
            if self.delims[o].count == 0 {
                self.unlink(o_node);
                self.remove_delim(o);
            }
            if self.delims[c].count == 0 {
                self.unlink(c_node);
                closer = self.delims[c].next;
                self.remove_delim(c);
            }
        }

        while self.top.is_some() && self.top != stack_bottom {
            self.remove_delim(self.top.unwrap());
        }
    }

    // ---- links and images ----

    fn push_bracket(&mut self, node: usize, index: usize, image: bool) {
        if let Some(last) = self.brackets.last_mut() {
            last.bracket_after = true;
        }
        self.brackets.push(Bracket {
            node,
            prev_delim: self.top,
            index,
            image,
            active: true,
            bracket_after: false,
        });
    }

    fn close_bracket(&mut self) {
        let start = self.pos;
        self.pos += 1;
        let Some(opener) = self.brackets.last() else {
            self.text(&self.s[start..start + 1]);
            return;
        };
        if !opener.active {
            self.brackets.pop();
            self.text(&self.s[start..start + 1]);
            return;
        }
        let (image, opener_node, opener_index, bracket_after, prev_delim) = (
            opener.image,
            opener.node,
            opener.index,
            opener.bracket_after,
            opener.prev_delim,
        );

        let target = self.inline_link().or_else(|| {
            // Reference link: full `[text][label]`, collapsed `[text][]` or shortcut `[text]`
            let after = self.pos;
            let (n, label) = match scan::link_label(&self.s[after..]) {
                Some((n, label)) => (n, Some(label)),
                None if self.s[after..].starts_with("[]") => (2, None),
                None => (0, None),
            };
            let label = match label {
                Some(label) => label,
                None if !bracket_after => &self.s[opener_index + 1..start],
                None => return None,
            };
            let def = self.refs.get(&scan::normalize_label(label))?;
            self.pos = after + n;
            Some((def.url.clone(), def.title.clone()))
        });

        let Some((url, title)) = target else {
            self.brackets.pop();
            self.pos = start + 1;
            self.text(&self.s[start..start + 1]);
            return;
        };

        let kind = if image {
            let line = self.line_at(opener_index);
            Kind::Image { url, title, line }
        } else {
            Kind::Link { url, title }
        };
        let link = self.nodes.len();
        self.nodes.push(Node::new(kind));
        self.move_siblings(opener_node, None, link);
        self.append_child(ROOT, link);
        self.process_emphasis(prev_delim);
        self.brackets.pop();
        self.unlink(opener_node);

        // No links inside links: deactivate all outer `[` (image `![` is not affected)
        if !image {
            for bracket in &mut self.brackets {
                if !bracket.image {
                    bracket.active = false;
                }
            }
        }
    }

    /// `](dest "title")`. On success, advance past the `)`.
    #[allow(clippy::type_complexity)]
    fn inline_link(&mut self) -> Option<(Cow<'s, str>, Option<Cow<'s, str>>)> {
        let s = self.s;
        if self.peek() != Some(b'(') {
            return None;
        }
        let mut i = scan::skip_spnl(s, self.pos + 1);
        let (n, raw_dest) = scan::link_destination(&s[i..])?;
        i += n;
        let before_title = i;
        i = scan::skip_spnl(s, i);
        let mut title = None;
        if i > before_title
            && let Some((n, raw)) = scan::link_title(&s[i..])
        {
            title = Some(scan::unescape(raw));
            i = scan::skip_spnl(s, i + n);
        }
        if s.as_bytes().get(i) != Some(&b')') {
            return None;
        }
        self.pos = i + 1;
        Some((scan::unescape(raw_dest), title))
    }

    // ---- output ----

    fn build(&mut self, parent: usize) -> Vec<Inline<'s>> {
        let mut out: Vec<Inline<'s>> = Vec::new();
        let mut cur = self.nodes[parent].first;
        while let Some(id) = cur {
            cur = self.nodes[id].next;
            let kind = std::mem::replace(&mut self.nodes[id].kind, Kind::Root);
            let inline = match kind {
                Kind::Text(text) => {
                    if text.is_empty() {
                        continue;
                    }
                    // Merge adjacent text; stays borrowed when contiguous in the source
                    if let Some(Inline::Text(prev)) = out.last_mut() {
                        merge_text(self.s, prev, text);
                        continue;
                    }
                    Inline::Text(text)
                }
                Kind::Code(code) => Inline::Code(code),
                Kind::Html(html) => Inline::Html(Cow::Borrowed(html)),
                Kind::SoftBreak => Inline::SoftBreak,
                Kind::HardBreak => Inline::HardBreak,
                Kind::Emph => Inline::Emph(self.build(id)),
                Kind::Strong => Inline::Strong(self.build(id)),
                Kind::Strike => Inline::Strike(self.build(id)),
                Kind::FootnoteRef(label) => Inline::FootnoteRef(Cow::Borrowed(label)),
                Kind::Math { tex, display, line } => Inline::Math {
                    tex: Cow::Borrowed(tex),
                    display,
                    line,
                },
                Kind::Link { url, title } => Inline::Link {
                    url,
                    title,
                    content: self.build(id),
                },
                Kind::Image { url, title, line } => Inline::Image {
                    url,
                    title,
                    alt: self.build(id),
                    line,
                },
                Kind::Root => unreachable!(),
            };
            out.push(inline);
        }
        out
    }
}

fn merge_text<'s>(src: &'s str, prev: &mut Cow<'s, str>, next: Cow<'s, str>) {
    if let (Cow::Borrowed(a), Cow::Borrowed(b)) = (&*prev, &next) {
        let base = src.as_ptr() as usize;
        let a_start = (a.as_ptr() as usize).wrapping_sub(base);
        let b_start = (b.as_ptr() as usize).wrapping_sub(base);
        // Both are slices of src and one ends where the other starts
        if a_start <= src.len() && b_start <= src.len() && a_start + a.len() == b_start {
            *prev = Cow::Borrowed(&src[a_start..b_start + b.len()]);
            return;
        }
    }
    prev.to_mut().push_str(&next);
}

/// Find a backtick run of exactly `run` characters, starting at `from`.
fn find_closing_backticks(b: &[u8], from: usize, run: usize) -> Option<usize> {
    let mut i = from;
    while i < b.len() {
        if b[i] == b'`' {
            let n = b[i..].iter().take_while(|&&c| c == b'`').count();
            if n == run {
                return Some(i);
            }
            i += n;
        } else {
            i += 1;
        }
    }
    None
}

/// Code span content: newlines become spaces; one space is stripped from each side when both sides have one (unless all spaces).
fn code_span_content(raw: &str) -> Cow<'_, str> {
    let strip = |s: &str| -> (usize, usize) {
        let b = s.as_bytes();
        if b.len() >= 2 && b[0] == b' ' && b[b.len() - 1] == b' ' && b.iter().any(|&c| c != b' ') {
            (1, b.len() - 1)
        } else {
            (0, b.len())
        }
    };
    if raw.contains('\n') {
        let replaced = raw.replace('\n', " ");
        let (from, to) = strip(&replaced);
        Cow::Owned(replaced[from..to].to_string())
    } else {
        let (from, to) = strip(raw);
        Cow::Borrowed(&raw[from..to])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Inline::*;

    fn text(s: &str) -> Inline<'_> {
        Text(Cow::Borrowed(s))
    }

    fn parse(s: &str) -> Vec<Inline<'_>> {
        parse_with(s, &HashMap::new(), Options::default())
    }

    fn parse_with<'s>(
        s: &'s str,
        links: &HashMap<String, LinkDef<'s>>,
        options: Options,
    ) -> Vec<Inline<'s>> {
        let footnotes = HashSet::from(["NOTE".to_string()]);
        parse_str(
            s,
            &Ctx {
                links,
                footnotes: &footnotes,
                options,
            },
            1,
        )
    }

    #[test]
    fn escapes_and_code_spans() {
        assert_eq!(parse(r"a\*b"), [text("a*b")]);
        assert_eq!(
            parse("x `` a`b `` y"),
            [text("x "), Code("a`b".into()), text(" y")]
        );
        assert_eq!(parse("`a\nb`"), [Code("a b".into())]);
        assert_eq!(parse("``` x"), [text("``` x")]);
    }

    #[test]
    fn line_breaks() {
        assert_eq!(parse("a \nb"), [text("a"), SoftBreak, text("b")]);
        assert_eq!(parse("a  \n  b"), [text("a"), HardBreak, text("b")]);
        assert_eq!(parse("a\\\nb"), [text("a"), HardBreak, text("b")]);
        assert_eq!(parse("a  "), [text("a")]);
        assert_eq!(parse("a\\"), [text("a\\")]);
    }

    #[test]
    fn emphasis() {
        assert_eq!(
            parse("*a* **b**"),
            [Emph(vec![text("a")]), text(" "), Strong(vec![text("b")])]
        );
        assert_eq!(parse("***a***"), [Emph(vec![Strong(vec![text("a")])])]);
        assert_eq!(
            parse("a*b*c"),
            [text("a"), Emph(vec![text("b")]), text("c")]
        );
        assert_eq!(parse("a_b_c"), [text("a_b_c")]);
    }

    #[test]
    fn links() {
        let mut refs = HashMap::new();
        refs.insert(
            "FOO".to_string(),
            LinkDef {
                url: "/u".into(),
                title: Some("t".into()),
            },
        );
        let link = |content| Link {
            url: "/u".into(),
            title: Some("t".into()),
            content,
        };
        assert_eq!(
            parse_with("[a](/u \"t\")", &refs, Options::default()),
            [link(vec![text("a")])]
        );
        assert_eq!(
            parse_with("[Foo]", &refs, Options::default()),
            [link(vec![text("Foo")])]
        );
        assert_eq!(
            parse_with("[x][foo]", &refs, Options::default()),
            [link(vec![text("x")])]
        );
        assert_eq!(
            parse_with("[bar]", &refs, Options::default()),
            [text("[bar]")]
        );
        assert_eq!(
            parse("![*a*](i.png)"),
            [Image {
                url: "i.png".into(),
                title: None,
                alt: vec![Emph(vec![text("a")])],
                line: 1,
            }]
        );
    }

    #[test]
    fn strikethrough() {
        let strike = |s| Strike(vec![text(s)]);
        assert_eq!(parse("~~a~~ ~b~"), [strike("a"), text(" "), strike("b")]);
        assert_eq!(parse("~~~a~~~"), [text("~~~a~~~")]);
        assert_eq!(parse("~a~~"), [text("~a~~")]);
        let commonmark = parse_with("~~a~~", &HashMap::new(), Options::commonmark());
        assert_eq!(commonmark, [text("~~a~~")]);
    }

    #[test]
    fn math() {
        let math = |tex| Math {
            tex: Cow::Borrowed(tex),
            display: false,
            line: 1,
        };
        assert_eq!(
            parse("$x^2$ 與 $a*b*c$"),
            [math("x^2"), text(" 與 "), math("a*b*c")]
        );
        assert_eq!(
            parse("$$ \\sum_i x_i $$"),
            [Math {
                tex: "\\sum_i x_i".into(),
                display: true,
                line: 1,
            }]
        );
        assert_eq!(parse("$\\$5$"), [math("\\$5")]);
        // Prices are not formulas
        assert_eq!(parse("價格 $5 到 $10"), [text("價格 $5 到 $10")]);
        assert_eq!(parse("$ x$"), [text("$ x$")]);
        assert_eq!(parse("\\$x$"), [text("$x$")]);
        let off = Options {
            math: false,
            ..Options::default()
        };
        assert_eq!(parse_with("$x$", &HashMap::new(), off), [text("$x$")]);
    }

    #[test]
    fn footnote_refs() {
        assert_eq!(parse("a[^note]"), [text("a"), FootnoteRef("note".into())]);
        assert_eq!(parse("a[^other]"), [text("a[^other]")]);
    }

    #[test]
    fn cjk_emphasis() {
        let strong = |s| Strong(vec![text(s)]);
        assert_eq!(
            parse("這是**「重點」**這樣"),
            [text("這是"), strong("「重點」"), text("這樣")]
        );
        let strict = parse_with(
            "這是**「重點」**這樣",
            &HashMap::new(),
            Options::commonmark(),
        );
        assert_eq!(strict, [text("這是**「重點」**這樣")]);
        // Non-CJK text keeps the standard rules
        assert_eq!(parse("a**\"b\"**c"), [text("a**\"b\"**c")]);
    }

    #[test]
    fn borrows_from_input() {
        let src = String::from("abc *d* `e`");
        let inlines = parse(&src);
        assert!(matches!(&inlines[0], Text(Cow::Borrowed(_))));
        assert!(matches!(&inlines[1], Emph(v) if matches!(&v[0], Text(Cow::Borrowed(_)))));
        assert!(matches!(&inlines[3], Code(Cow::Borrowed(_))));
    }
}
