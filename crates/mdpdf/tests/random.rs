//! Escaping property test: random text full of Markdown, Typst and CJK special characters must
//! always compile. Any character that leaked into Typst markup unescaped would show up as a
//! compile error or warning. Seeded, so failures are reproducible.

use std::path::Path;

use mdpdf::fonts::{self, FontOptions};

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

/// Markdown syntax, Typst markup and code characters, and CJK text.
const PIECES: &[&str] = &[
    "#",
    "#set page(width: 1pt)",
    "#let",
    "$",
    "$x$",
    "@",
    "@ref",
    "<label>",
    "[",
    "]",
    "{",
    "}",
    "(",
    ")",
    "\"",
    "\\",
    "\\\"",
    "/",
    "//",
    "/*",
    "*/",
    "*",
    "_",
    "`",
    "~",
    "=",
    "+",
    "-",
    "'",
    "<",
    ">",
    "&",
    "&amp;",
    ";",
    ":",
    ",",
    ".",
    "!",
    "?",
    "|",
    "^",
    "%",
    "\n",
    "\n\n",
    " ",
    "\t",
    "中文",
    "「引號」",
    "，",
    "。",
    "a",
    "Z",
    "9",
    "é",
    "😀",
    "- ",
    "> ",
    "# ",
    "```",
    "**",
    "[^n]",
    "[^n]: 註腳",
    "| a | b |\n|---|---|\n",
    "- [x] ",
    "<!-- pagebreak -->",
    "$$\\frac{a}{b}$$",
];

#[test]
#[cfg(feature = "embed-cjk")]
fn random_text_always_compiles() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .canonicalize()
        .unwrap();
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    for round in 0..200 {
        let len = 20 + rng.below(80);
        let markdown: String = (0..len).map(|_| PIECES[rng.below(PIECES.len())]).collect();
        let fonts = fonts::load(&FontOptions {
            system_fonts: false,
            ..Default::default()
        })
        .unwrap();
        let options = md2typst::Options {
            base_dir: Some(dir.clone()),
            ..Default::default()
        };
        let rendered = mdpdf::render(&markdown, &dir.join("main.typ"), options, fonts).unwrap();
        if let Err(errors) = &rendered.result.output {
            panic!("round {round}: {markdown:?} failed to compile: {errors:#?}");
        }
        assert!(
            rendered.result.warnings.is_empty(),
            "round {round}: {markdown:?} produced Typst warnings: {:#?}",
            rendered.result.warnings
        );
    }
}
