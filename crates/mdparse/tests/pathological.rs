//! Pathological inputs, ported from cmark's `test/pathological_tests.py` and extended with
//! mdpdf's own syntax. Each case must finish quickly (no quadratic or exponential blowup) and
//! must not overflow the stack.

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// Time limit per case. Generous because tests build this crate without optimizations;
/// a quadratic algorithm on these inputs takes minutes, not seconds.
const LIMIT: Duration = Duration::from_secs(10);
/// Stack size of the main thread on Linux, so deep recursion fails here as it would in the CLI.
const STACK: usize = 8 * 1024 * 1024;

fn cases() -> Vec<(&'static str, String)> {
    let r = |s: &str, n: usize| s.repeat(n);
    vec![
        (
            "nested strong emph",
            format!("{}b{}", r("*a **a ", 65000), r(" a** a*", 65000)),
        ),
        ("many emph closers with no openers", r("a_ ", 65000)),
        ("many emph openers with no closers", r("_a ", 65000)),
        ("many link closers with no openers", r("a]", 65000)),
        ("many link openers with no closers", r("[a", 65000)),
        ("mismatched openers and closers", r("*a_ ", 50000)),
        (
            "openers and closers multiple of 3",
            format!("a**b{}", r("c* ", 50000)),
        ),
        ("link openers and emph closers", r("[ a_", 50000)),
        ("pattern [ (]( repeated", r("[ (](", 80000)),
        (
            "nested brackets",
            format!("{}a{}", r("[", 50000), r("]", 50000)),
        ),
        ("nested block quotes", format!("{}a", r("> ", 50000))),
        (
            "deeply nested lists",
            (0..1000)
                .map(|i| format!("{}* a\n", "  ".repeat(i)))
                .collect(),
        ),
        ("U+0000 in input", "abc\u{0}de\u{0}".into()),
        (
            "backticks",
            (1..5000).map(|i| format!("e{}", "`".repeat(i))).collect(),
        ),
        ("unclosed links A", r("[a](<b", 30000)),
        ("unclosed links B", r("[a](b", 30000)),
        ("unclosed link titles", r("[a](b \"c", 30000)),
        (
            "many link references",
            (0..20000)
                .map(|i| format!("[{i}]: /u\n"))
                .collect::<String>()
                + &r("[1]", 20000),
        ),
        (
            "reference label too long",
            format!("[{}]\n\n[{}]: /u", r("a", 50000), r("a", 50000)),
        ),
        ("many html comments", r("a <!-- ", 30000)),
        ("many cdata", r("a <![CDATA[", 30000)),
        ("many processing instructions", r("a <?", 30000)),
        ("many declarations", r("a <!A ", 30000)),
        ("many entities", r("&amp;&#12;&#x1F;&unknown;", 30000)),
        (
            "many table columns",
            format!(
                "{}\n{}\n{}\n",
                r("|a", 10000),
                r("|-", 10000),
                r("|b", 10000)
            ),
        ),
        (
            "many table rows",
            format!("|a|b|\n|-|-|\n{}", r("|x|y|\n", 30000)),
        ),
        ("many dollar signs", r("$a", 50000)),
        (
            "many math spans across lines",
            r("$a$ ![i](x.png)\n", 30000),
        ),
        ("unclosed display math", format!("$$\n{}", r("x\n", 30000))),
        (
            "many footnote refs",
            format!("{}\n\n[^a]: note\n", r("[^a]", 30000)),
        ),
        ("nested footnote definitions", r("[^a]: ", 20000)),
        ("strikethrough runs", r("~~a ~b ", 50000)),
        ("task list items", r("- [ ] a\n", 30000)),
        ("page break markers", r("<!-- pagebreak -->\n", 30000)),
        (
            "front matter only",
            format!("---\n{}---\n", r("k: v\n", 30000)),
        ),
        ("long line of spaces", r(" ", 1_000_000) + "a"),
        ("many tabs", r("\t>\t-\t", 30000)),
    ]
}

/// Run `f` on a thread with the CLI's stack size and a time limit.
fn check(name: &str, input: String, f: fn(&str)) {
    let (tx, rx) = mpsc::channel();
    let start = Instant::now();
    thread::Builder::new()
        .stack_size(STACK)
        .spawn(move || {
            f(&input);
            let _ = tx.send(());
        })
        .unwrap();
    match rx.recv_timeout(LIMIT) {
        Ok(()) => {}
        Err(mpsc::RecvTimeoutError::Timeout) => panic!("{name}: took longer than {LIMIT:?}"),
        Err(mpsc::RecvTimeoutError::Disconnected) => panic!("{name}: panicked"),
    }
    eprintln!("{:>8.1?}  {name}", start.elapsed());
}

#[test]
fn pathological_inputs_parse_quickly() {
    for (name, input) in cases() {
        check(name, input, |s| {
            let doc = mdparse::parse(s);
            std::hint::black_box(mdparse::html::render(&doc));
        });
    }
}
