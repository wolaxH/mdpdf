//! 區塊與 inline 階段共用的小型掃描器。
//!
//! 每個函式從 `s` 的開頭嘗試比對，成功時回傳消耗的位元組數。

use std::borrow::Cow;

use crate::ast::LinkDef;
use crate::entities::ENTITIES;
use crate::unicode::{PUNCTUATION, WHITESPACE};

pub fn is_ascii_punct(b: u8) -> bool {
    b.is_ascii_punctuation()
}

pub fn is_space_or_tab(b: u8) -> bool {
    b == b' ' || b == b'\t'
}

fn in_ranges(table: &[(char, char)], c: char) -> bool {
    table
        .binary_search_by(|&(lo, hi)| {
            if hi < c {
                std::cmp::Ordering::Less
            } else if lo > c {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// Unicode 標點字元：P（標點）或 S（符號）類別。
pub fn is_unicode_punct(c: char) -> bool {
    if c.is_ascii() {
        c.is_ascii_punctuation()
    } else {
        in_ranges(PUNCTUATION, c)
    }
}

/// 中日韓文字或全形標點（寬鬆強調與軟換行判斷用）。
pub fn is_cjk(c: char) -> bool {
    matches!(c,
        '\u{2E80}'..='\u{9FFF}'      // 部首、標點、假名、注音、CJK 統一表意文字
        | '\u{AC00}'..='\u{D7AF}'    // 韓文音節
        | '\u{F900}'..='\u{FAFF}'    // 相容表意文字
        | '\u{FE30}'..='\u{FE4F}'    // 相容形式（直排標點）
        | '\u{FF00}'..='\u{FFEF}'    // 全形字元
        | '\u{20000}'..='\u{3FFFF}'  // 擴充 B 之後
    )
}

/// Unicode 空白字元：Zs 類別加上 tab、換行、換頁、歸位。
pub fn is_unicode_whitespace(c: char) -> bool {
    in_ranges(WHITESPACE, c)
}

/// 行尾只剩空白（或已到結尾）。
pub fn is_blank(s: &str) -> bool {
    s.bytes().all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
}

/// 處理反斜線跳脫與字元參照：`\` 後接 ASCII 標點時只保留該標點，`&...;` 解碼。
pub fn unescape(s: &str) -> Cow<'_, str> {
    if !s.contains(['\\', '&']) {
        return Cow::Borrowed(s);
    }
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut last = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if bytes.get(i + 1).is_some_and(|&c| is_ascii_punct(c)) => {
                out.push_str(&s[last..i]);
                last = i + 1;
                i += 2;
            }
            b'&' => match entity(&s[i..]) {
                Some((n, decoded)) => {
                    out.push_str(&s[last..i]);
                    out.push_str(&decoded);
                    i += n;
                    last = i;
                }
                None => i += 1,
            },
            _ => i += 1,
        }
    }
    out.push_str(&s[last..]);
    Cow::Owned(out)
}

/// 字元參照：`&name;`、`&#123;`、`&#x1F;`。回傳（消耗長度, 解碼後文字）。
pub fn entity(s: &str) -> Option<(usize, Cow<'static, str>)> {
    let b = s.as_bytes();
    if b.first() != Some(&b'&') {
        return None;
    }
    if b.get(1) == Some(&b'#') {
        let hex = matches!(b.get(2), Some(b'x' | b'X'));
        let start = if hex { 3 } else { 2 };
        let (radix, max) = if hex { (16, 6) } else { (10, 7) };
        let n = b[start..]
            .iter()
            .take_while(|c| c.is_ascii_digit() || (hex && c.is_ascii_hexdigit()))
            .count();
        if n == 0 || n > max || b.get(start + n) != Some(&b';') {
            return None;
        }
        let code = u32::from_str_radix(&s[start..start + n], radix).ok()?;
        // 0、代理字元與超出範圍的碼位一律替換為 U+FFFD
        let c = char::from_u32(code)
            .filter(|&c| c != '\0')
            .unwrap_or('\u{FFFD}');
        return Some((start + n + 1, Cow::Owned(c.to_string())));
    }
    let n = b[1..]
        .iter()
        .take_while(|c| c.is_ascii_alphanumeric())
        .count();
    if !(2..=32).contains(&n) || !b[1].is_ascii_alphabetic() || b.get(1 + n) != Some(&b';') {
        return None;
    }
    let name = &s[1..1 + n];
    let idx = ENTITIES.binary_search_by(|(k, _)| k.cmp(&name)).ok()?;
    Some((n + 2, Cow::Borrowed(ENTITIES[idx].1)))
}

/// 連結標籤正規化：去頭尾空白、內部連續空白縮成一格、case fold。
pub fn normalize_label(label: &str) -> String {
    let collapsed = label
        .split([' ', '\t', '\r', '\n'])
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    collapsed.to_lowercase().to_uppercase()
}

/// 跳過空白與至多一個換行。
pub fn skip_spnl(s: &str, mut i: usize) -> usize {
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
    // 空的目的地只在 inline 連結 `[a]()` 中合法
    if (i == 0 && b.first() != Some(&b')')) || depth != 0 {
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

/// 行內原始 HTML：開始／結束標籤、註解、處理指令、宣告或 CDATA。回傳消耗長度。
pub fn inline_html(s: &str) -> Option<usize> {
    let find_end = |from: usize, end: &str| s[from..].find(end).map(|i| from + i + end.len());
    if let Some(rest) = s.strip_prefix("<!--") {
        // `<!-->` 與 `<!--->` 也是合法（空）註解
        if rest.starts_with('>') {
            return Some(5);
        }
        if rest.starts_with("->") {
            return Some(6);
        }
        return find_end(4, "-->");
    }
    if s.starts_with("<?") {
        return find_end(2, "?>");
    }
    if s.starts_with("<![CDATA[") {
        return find_end(9, "]]>");
    }
    if s.starts_with("<!") && s.as_bytes().get(2).is_some_and(u8::is_ascii_alphabetic) {
        return find_end(2, ">");
    }
    open_tag(s).or_else(|| closing_tag(s))
}

/// URI autolink `<scheme:...>`，回傳（消耗長度, URI）。
pub fn autolink_uri(s: &str) -> Option<(usize, &str)> {
    let b = s.as_bytes();
    if b.first() != Some(&b'<') || !b.get(1)?.is_ascii_alphabetic() {
        return None;
    }
    let scheme = b[1..]
        .iter()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'.' | b'-'))
        .count();
    if !(2..=32).contains(&scheme) || b.get(1 + scheme) != Some(&b':') {
        return None;
    }
    let mut i = 2 + scheme;
    while i < b.len() {
        match b[i] {
            b'>' => return Some((i + 1, &s[1..i])),
            b'<' => return None,
            c if c <= b' ' => return None,
            _ => i += 1,
        }
    }
    None
}

/// Email autolink `<user@host>`，回傳（消耗長度, 位址）。
pub fn autolink_email(s: &str) -> Option<(usize, &str)> {
    let b = s.as_bytes();
    if b.first() != Some(&b'<') {
        return None;
    }
    let local = b[1..]
        .iter()
        .take_while(|&&c| c.is_ascii_alphanumeric() || b".!#$%&'*+/=?^_`{|}~-".contains(&c))
        .count();
    if local == 0 || b.get(1 + local) != Some(&b'@') {
        return None;
    }
    let mut i = 2 + local;
    loop {
        // 網域標籤：英數開頭與結尾，中間可有連字號，最長 63
        let label = b[i..]
            .iter()
            .take_while(|c| c.is_ascii_alphanumeric() || **c == b'-')
            .count();
        if label == 0 || label > 63 || b[i] == b'-' || b[i + label - 1] == b'-' {
            return None;
        }
        i += label;
        match b.get(i) {
            Some(b'.') => i += 1,
            Some(b'>') => return Some((i + 1, &s[1..i])),
            _ => return None,
        }
    }
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
