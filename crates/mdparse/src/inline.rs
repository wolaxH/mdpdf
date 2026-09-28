//! Inline 階段：把段落或標題的文字內容解析成 [`Inline`] 序列。
//!
//! 目前支援反斜線跳脫、行內程式碼、軟換行與硬換行。強調、連結、圖片、autolink、
//! 原始 HTML 與 entity 在 M2 加入，在那之前都當成一般文字。

use std::borrow::Cow;

use crate::ast::Inline;
use crate::scan;

/// 解析一段內容。借用的內容產生借用的節點，否則整棵結果轉為 owned。
pub fn parse(content: Cow<'_, str>) -> Vec<Inline<'_>> {
    match content {
        Cow::Borrowed(s) => parse_str(s),
        Cow::Owned(s) => parse_str(&s).into_iter().map(Inline::into_owned).collect(),
    }
}

pub fn parse_str<'s>(s: &'s str) -> Vec<Inline<'s>> {
    let s = s.trim_matches([' ', '\t', '\n']);
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut text_start = 0;
    let mut i = 0;

    let flush = |out: &mut Vec<Inline<'s>>, from: usize, to: usize| {
        if from < to {
            out.push(Inline::Text(Cow::Borrowed(&s[from..to])));
        }
    };

    while i < b.len() {
        match b[i] {
            b'\\' if b.get(i + 1) == Some(&b'\n') => {
                flush(&mut out, text_start, i);
                out.push(Inline::HardBreak);
                i = skip_spaces(b, i + 2);
                text_start = i;
            }
            b'\\' if b.get(i + 1).is_some_and(|&c| scan::is_ascii_punct(c)) => {
                flush(&mut out, text_start, i);
                out.push(Inline::Text(Cow::Borrowed(&s[i + 1..i + 2])));
                i += 2;
                text_start = i;
            }
            b'`' => {
                let run = b[i..].iter().take_while(|&&c| c == b'`').count();
                match find_closing_backticks(b, i + run, run) {
                    Some(close) => {
                        flush(&mut out, text_start, i);
                        out.push(Inline::Code(code_span_content(&s[i + run..close])));
                        i = close + run;
                        text_start = i;
                    }
                    // 找不到對應的結尾時，整串反引號都是一般文字
                    None => i += run,
                }
            }
            b'\n' => {
                let kept = s[text_start..i].trim_end_matches(' ').len();
                let hard = i - text_start - kept >= 2;
                flush(&mut out, text_start, text_start + kept);
                out.push(if hard {
                    Inline::HardBreak
                } else {
                    Inline::SoftBreak
                });
                i = skip_spaces(b, i + 1);
                text_start = i;
            }
            _ => i += 1,
        }
    }
    flush(&mut out, text_start, b.len());
    out
}

fn skip_spaces(b: &[u8], mut i: usize) -> usize {
    while b.get(i) == Some(&b' ') {
        i += 1;
    }
    i
}

/// 從 `from` 開始尋找長度恰好為 `run` 的反引號串。
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

/// 行內程式碼內容：換行轉為空白；前後都有空白（且不全是空白）時各去掉一個。
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

    #[test]
    fn escapes_and_code_spans() {
        assert_eq!(parse_str(r"a\*b"), [text("a"), text("*"), text("b")]);
        assert_eq!(
            parse_str("x `` a`b `` y"),
            [text("x "), Code("a`b".into()), text(" y")]
        );
        assert_eq!(parse_str("`a\nb`"), [Code("a b".into())]);
        assert_eq!(parse_str("``` x"), [text("``` x")]);
        assert_eq!(parse_str(r"\`a`"), [text("`"), text("a`")]);
    }

    #[test]
    fn line_breaks() {
        assert_eq!(parse_str("a \nb"), [text("a"), SoftBreak, text("b")]);
        assert_eq!(parse_str("a  \n  b"), [text("a"), HardBreak, text("b")]);
        assert_eq!(parse_str("a\\\nb"), [text("a"), HardBreak, text("b")]);
        // 段落結尾的空白或反斜線不構成硬換行
        assert_eq!(parse_str("a  "), [text("a")]);
        assert_eq!(parse_str("a\\"), [text("a\\")]);
    }

    #[test]
    fn borrows_from_input() {
        let src = String::from("abc `d`");
        let inlines = parse_str(&src);
        assert!(matches!(&inlines[0], Text(Cow::Borrowed(_))));
        assert!(matches!(&inlines[1], Code(Cow::Borrowed(_))));
    }
}
