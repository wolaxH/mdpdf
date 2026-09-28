//! Randomized robustness test: mutated spec examples and random Markdown-ish text must never
//! make the parser or the HTML renderer panic. Seeded, so failures are reproducible.

use std::path::Path;

use serde::Deserialize;

/// xorshift64*: tiny, deterministic, good enough for test inputs.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

const PIECES: &[&str] = &[
    "*",
    "**",
    "_",
    "__",
    "~",
    "~~",
    "`",
    "```",
    "[",
    "]",
    "(",
    ")",
    "![",
    "<",
    ">",
    "&",
    "&amp;",
    "&#x41;",
    "\\",
    "\n",
    "\n\n",
    " ",
    "  ",
    "\t",
    "#",
    "# ",
    "- ",
    "1. ",
    "> ",
    "|",
    "|-|",
    ":",
    "$",
    "$$",
    "[^a]",
    "[^a]: ",
    "---",
    "===",
    "<!--",
    "-->",
    "<div>",
    "</div>",
    "http://a.b",
    "<a@b.c>",
    "[ ]",
    "[x]",
    "\"",
    "'",
    "中文",
    "「重點」",
    "。",
    "a",
    "b",
    "0",
    "<!-- pagebreak -->",
];

fn random_markdown(rng: &mut Rng) -> String {
    let len = rng.below(60);
    (0..len).map(|_| PIECES[rng.below(PIECES.len())]).collect()
}

/// Delete, duplicate or splice random character ranges.
fn mutate(rng: &mut Rng, input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    if chars.is_empty() {
        return String::new();
    }
    let a = rng.below(chars.len());
    let b = (a + rng.below(8)).min(chars.len());
    let mut out: Vec<char> = Vec::with_capacity(chars.len() + 8);
    match rng.below(3) {
        0 => out.extend(chars[..a].iter().chain(&chars[b..])),
        1 => out.extend(chars[..b].iter().chain(&chars[a..])),
        _ => {
            out.extend(&chars[..a]);
            out.extend(PIECES[rng.below(PIECES.len())].chars());
            out.extend(&chars[a..]);
        }
    }
    out.into_iter().collect()
}

#[derive(Deserialize)]
struct Example {
    markdown: String,
}

fn check(input: &str) {
    let result = std::panic::catch_unwind(|| {
        for options in [mdparse::Options::default(), mdparse::Options::commonmark()] {
            let doc = mdparse::parse_with(input, options);
            std::hint::black_box(mdparse::html::render(&doc));
        }
    });
    assert!(result.is_ok(), "parser panicked on {input:?}");
}

#[test]
fn random_inputs_never_panic() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/spec/commonmark-0.31.2.json");
    let examples: Vec<Example> =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);

    for _ in 0..20_000 {
        check(&random_markdown(&mut rng));
    }
    for _ in 0..20_000 {
        let a = &examples[rng.below(examples.len())].markdown;
        let b = &examples[rng.below(examples.len())].markdown;
        let mut input = mutate(&mut rng, a);
        if rng.below(2) == 0 {
            input.push_str(b);
            input = mutate(&mut rng, &input);
        }
        check(&input);
    }
}
