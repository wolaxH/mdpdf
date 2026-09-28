//! 區塊與 inline 階段共用的小型掃描器。
//!
//! 每個函式從 `s` 的開頭嘗試比對，成功時回傳消耗的位元組數。

use std::borrow::Cow;

use crate::ast::LinkDef;

pub fn is_ascii_punct(b: u8) -> bool {
    b.is_ascii_punctuation()
}

pub fn is_space_or_tab(b: u8) -> bool {
    b == b' ' || b == b'\t'
}

/// 行尾只剩空白（或已到結尾）。
pub fn is_blank(s: &str) -> bool {
    s.bytes().all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
}

/// 處理反斜線跳脫：`\` 後接 ASCII 標點時只保留該標點。
pub fn unescape(s: &str) -> Cow<'_, str> {
    if !s.contains('\\') {
        return Cow::Borrowed(s);
    }
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut last = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() && is_ascii_punct(bytes[i + 1]) {
            out.push_str(&s[last..i]);
            last = i + 1;
            i += 2;
        } else {
            i += 1;
        }
    }
    out.push_str(&s[last..]);
    Cow::Owned(out)
}

/// 連結標籤正規化：去頭尾空白、內部連續空白縮成一格、case fold。
pub fn normalize_label(label: &str) -> String {
    let collapsed = label.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.to_lowercase().to_uppercase()
}

/// 跳過空白與至多一個換行。
fn skip_spnl(s: &str, mut i: usize) -> usize {
    let b = s.as_bytes();
    while i < b.len() && is_space_or_tab(b[i]) {
        i += 1;
    }
    if i < b.len() && b[i] == b'\n' {
        i += 1;
        while i < b.len() && is_space_or_tab(b[i]) {
            i += 1;
        }
    }
    i
}

/// 連結標籤 `[...]`，回傳（消耗長度, 內部文字）。
pub fn link_label(s: &str) -> Option<(usize, &str)> {
    let b = s.as_bytes();
    if b.first() != Some(&b'[') {
        return None;
    }
    let mut i = 1;
    while i < b.len() {
        match b[i] {
            b'\\' if i + 1 < b.len() => i += 2,
            b'[' => return None,
            b']' => {
                let inner = &s[1..i];
                if i - 1 > 999 || is_blank(inner) {
                    return None;
                }
                return Some((i + 1, inner));
            }
            _ => i += 1,
        }
    }
    None
}

/// 連結目的地：`<...>` 或不含空白、括號平衡的字串。回傳（消耗長度, 原始文字）。
pub fn link_destination(s: &str) -> Option<(usize, &str)> {
    let b = s.as_bytes();
    if b.first() == Some(&b'<') {
        let mut i = 1;
        while i < b.len() {
            match b[i] {
                b'>' => return Some((i + 1, &s[1..i])),
                b'<' | b'\n' => return None,
                b'\\' if i + 1 < b.len() => i += 2,
                _ => i += 1,
            }
        }
        return None;
    }

    let mut depth = 0usize;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\\' if i + 1 < b.len() && is_ascii_punct(b[i + 1]) => i += 2,
            b'(' => {
                depth += 1;
                if depth > 32 {
                    return None;
                }
                i += 1;
            }
            b')' => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
                i += 1;
            }
            c if c <= b' ' || c == 0x7f => break,
            _ => i += 1,
        }
    }
    if i == 0 || depth != 0 {
        return None;
    }
    Some((i, &s[..i]))
}

/// 連結標題：`"..."`、`'...'` 或 `(...)`。回傳（消耗長度, 內部原始文字）。
pub fn link_title(s: &str) -> Option<(usize, &str)> {
    let b = s.as_bytes();
    let close = match b.first()? {
        b'"' => b'"',
        b'\'' => b'\'',
        b'(' => b')',
        _ => return None,
    };
    let mut i = 1;
    while i < b.len() {
        match b[i] {
            b'\\' if i + 1 < b.len() => i += 2,
            c if c == close => return Some((i + 1, &s[1..i])),
            b'(' if close == b')' => return None,
            _ => i += 1,
        }
    }
    None
}

/// 解析段落開頭的一個連結參照定義，回傳（消耗長度, 正規化標籤, 定義）。
pub fn link_reference_def(s: &str) -> Option<(usize, String, LinkDef<'_>)> {
    let b = s.as_bytes();
    let (n, label) = link_label(s)?;
    let mut i = n;
    if b.get(i) != Some(&b':') {
        return None;
    }
    i = skip_spnl(s, i + 1);

    let (n, raw_dest) = link_destination(&s[i..])?;
    i += n;

    let at_line_end = |mut j: usize| {
        while j < b.len() && is_space_or_tab(b[j]) {
            j += 1;
        }
        (j == b.len() || b[j] == b'\n').then(|| (j + 1).min(b.len()))
    };

    let before_title = i;
    let after_space = skip_spnl(s, i);
    let mut title = None;
    let mut end = None;
    if after_space > before_title
        && let Some((n, raw)) = link_title(&s[after_space..])
    {
        end = at_line_end(after_space + n);
        if end.is_some() {
            title = Some(unescape(raw));
        }
    }
    let end = match end {
        Some(end) => end,
        // 標題不合法時，若目的地之後就是行尾，定義仍成立（標題行留給段落）。
        None => at_line_end(before_title)?,
    };

    let label = normalize_label(label);
    if label.is_empty() {
        return None;
    }
    Some((
        end,
        label,
        LinkDef {
            url: unescape(raw_dest),
            title,
        },
    ))
}

/// HTML 區塊的開始條件（CommonMark 4.6），回傳類型 1–7。
pub fn html_block_start(s: &str, can_interrupt_paragraph: bool) -> Option<u8> {
    let b = s.as_bytes();
    if b.first() != Some(&b'<') {
        return None;
    }
    let rest = &s[1..];
    let lower = rest
        .get(..rest.len().min(12))
        .unwrap_or(rest)
        .to_ascii_lowercase();

    for tag in ["script", "pre", "textarea", "style"] {
        if lower.starts_with(tag) {
            match rest.as_bytes().get(tag.len()) {
                None | Some(b' ' | b'\t' | b'>' | b'\n') => return Some(1),
                _ => {}
            }
        }
    }
    if rest.starts_with("!--") {
        return Some(2);
    }
    if rest.starts_with('?') {
        return Some(3);
    }
    if rest.starts_with('!') && rest.as_bytes().get(1).is_some_and(u8::is_ascii_alphabetic) {
        return Some(4);
    }
    if rest.starts_with("![CDATA[") {
        return Some(5);
    }

    let name_start = usize::from(rest.starts_with('/'));
    let name_len = rest[name_start..]
        .bytes()
        .take_while(|c| c.is_ascii_alphanumeric())
        .count();
    let name = rest[name_start..name_start + name_len].to_ascii_lowercase();
    if BLOCK_TAGS.binary_search(&name.as_str()).is_ok() {
        let after = &rest[name_start + name_len..];
        if after.is_empty() || after.starts_with([' ', '\t', '\n', '>']) || after.starts_with("/>")
        {
            return Some(6);
        }
    }

    if !can_interrupt_paragraph {
        let n = open_tag(s).or_else(|| closing_tag(s))?;
        if is_blank(&s[n..]) {
            return Some(7);
        }
    }
    None
}

/// HTML 區塊類型 1–5 的結束條件是否出現在這一行。
pub fn html_block_end(kind: u8, line: &str) -> bool {
    match kind {
        1 => {
            let lower = line.to_ascii_lowercase();
            ["</script>", "</pre>", "</textarea>", "</style>"]
                .iter()
                .any(|t| lower.contains(t))
        }
        2 => line.contains("-->"),
        3 => line.contains("?>"),
        4 => line.contains('>'),
        5 => line.contains("]]>"),
        _ => false,
    }
}

/// 類型 6 的區塊層級標籤（已排序，供二分搜尋）。
const BLOCK_TAGS: &[&str] = &[
    "address",
    "article",
    "aside",
    "base",
    "basefont",
    "blockquote",
    "body",
    "caption",
    "center",
    "col",
    "colgroup",
    "dd",
    "details",
    "dialog",
    "dir",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "frame",
    "frameset",
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
    "iframe",
    "legend",
    "li",
    "link",
    "main",
    "menu",
    "menuitem",
    "nav",
    "noframes",
    "ol",
    "optgroup",
    "option",
    "p",
    "param",
    "search",
    "section",
    "summary",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "title",
    "tr",
    "track",
    "ul",
];

fn tag_name(b: &[u8], mut i: usize) -> Option<usize> {
    if !b.get(i)?.is_ascii_alphabetic() {
        return None;
    }
    while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'-') {
        i += 1;
    }
    Some(i)
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && b[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

/// 開始標籤 `<tag attr="v" ...>`，回傳消耗長度。
pub fn open_tag(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    if b.first() != Some(&b'<') {
        return None;
    }
    let mut i = tag_name(b, 1)?;
    loop {
        let ws = skip_ws(b, i);
        // 屬性前必須有空白
        if ws > i && ws < b.len() && (b[ws].is_ascii_alphabetic() || b[ws] == b'_' || b[ws] == b':')
        {
            i = ws + 1;
            while i < b.len()
                && (b[i].is_ascii_alphanumeric() || matches!(b[i], b'_' | b'.' | b':' | b'-'))
            {
                i += 1;
            }
            let eq = skip_ws(b, i);
            if b.get(eq) == Some(&b'=') {
                let v = skip_ws(b, eq + 1);
                i = attribute_value(b, v)?;
            }
            continue;
        }
        i = ws;
        if b.get(i) == Some(&b'/') {
            i += 1;
        }
        return (b.get(i) == Some(&b'>')).then_some(i + 1);
    }
}

fn attribute_value(b: &[u8], i: usize) -> Option<usize> {
    match *b.get(i)? {
        q @ (b'"' | b'\'') => {
            let end = b[i + 1..].iter().position(|&c| c == q)?;
            Some(i + 1 + end + 1)
        }
        _ => {
            let n = b[i..]
                .iter()
                .take_while(|&&c| !c.is_ascii_whitespace() && !b"\"'=<>`".contains(&c))
                .count();
            (n > 0).then_some(i + n)
        }
    }
}

/// 結束標籤 `</tag>`，回傳消耗長度。
pub fn closing_tag(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    if !s.starts_with("</") {
        return None;
    }
    let i = skip_ws(b, tag_name(b, 2)?);
    (b.get(i) == Some(&b'>')).then_some(i + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_definitions() {
        let (n, label, def) = link_reference_def("[Foo  Bar]: /url \"title\"\nrest").unwrap();
        assert_eq!(&"[Foo  Bar]: /url \"title\"\nrest"[n..], "rest");
        assert_eq!(label, "FOO BAR");
        assert_eq!(def.url, "/url");
        assert_eq!(def.title.as_deref(), Some("title"));

        // 標題之後還有文字：標題不成立，但定義本身仍成立
        let (n, _, def) = link_reference_def("[a]: /u\n\"t\" x").unwrap();
        assert_eq!(n, 8);
        assert_eq!(def.title, None);

        assert!(link_reference_def("[a]: /u \"t\" x").is_none());
        assert!(link_reference_def("[a]:").is_none());
        assert!(link_reference_def("[]: /u").is_none());
    }

    #[test]
    fn html_block_kinds() {
        assert_eq!(html_block_start("<pre>", true), Some(1));
        assert_eq!(html_block_start("<!-- x", true), Some(2));
        assert_eq!(html_block_start("<div class=\"a\">", true), Some(6));
        assert_eq!(html_block_start("</DIV>", true), Some(6));
        assert_eq!(html_block_start("<span a='1'>", false), Some(7));
        assert_eq!(html_block_start("<span a='1'>", true), None);
        assert_eq!(html_block_start("<span> text", false), None);
    }
}
