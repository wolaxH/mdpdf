# mdpdf

Convert Markdown reports to PDF with a single binary. No pandoc, browser or LaTeX
installation is needed: mdpdf has its own CommonMark parser and embeds the
[Typst](https://typst.app) compiler for layout.

```sh
mdpdf report.md            # writes report.pdf
```

- **CommonMark 0.31.2** (all 652 spec examples pass) plus GitHub Flavored Markdown: tables,
  strikethrough, task lists and footnotes
- **CJK first**: a Traditional Chinese font is embedded, soft line breaks between CJK
  characters do not add spaces, and `**「重點」**` works next to CJK punctuation
- **LaTeX math** with `$...$`, `$$...$$` and ```` ```math ```` blocks
- **Syntax highlighting**, images (PNG, JPEG, GIF, WebP, SVG, PDF), table of contents,
  numbered headings, custom templates
- **Fast**: a 30-page report converts in about 150 ms
- **Forgiving**: a broken image or formula becomes a placeholder with a warning pointing at
  the Markdown line, instead of failing the whole document

## Installation

mdpdf is not yet published. Build it from source with a recent stable Rust toolchain:

```sh
git clone <repository> mdpdf && cd mdpdf
scripts/fetch-fonts.sh               # downloads Noto Sans TC into assets/fonts/
cargo install --path crates/mdpdf
```

`scripts/fetch-fonts.sh` is needed because the embedded fonts are not stored in git. To build
without them (smaller binary, CJK text then needs an installed CJK font), use
`cargo install --path crates/mdpdf --no-default-features`.

An AUR `PKGBUILD` is in [`packaging/aur/mdpdf-git`](packaging/aur/mdpdf-git).

## Usage

```sh
mdpdf report.md                              # report.pdf next to report.md
mdpdf report.md -o out/q3.pdf --toc          # table of contents
mdpdf report.md --paper a5 --margin 2cm --number-headings
mdpdf report.md --cjk-font "LXGW WenKai TC"  # any installed font; see `mdpdf fonts`
mdpdf report.md --watch                      # rebuild on every save
cat notes.md | mdpdf - -o notes.pdf          # read stdin
mdpdf fonts                                  # list font names
```

| Option | Description |
| --- | --- |
| `-o, --output <FILE>` | Output path (default: input name with `.pdf`; `-` for stdout) |
| `-w, --watch` | Rebuild when the input, its images, the template, code theme or config change |
| `--strict` | Treat warnings (missing images, missing fonts, ...) as errors |
| `--emit-typst <FILE>` | Also write the generated Typst source |
| `--font <NAME>` | Body font for Latin text |
| `--cjk-font <NAME>` | Font for CJK text (default: embedded Noto Sans TC) |
| `--mono-font <NAME>` | Font for code (default: DejaVu Sans Mono) |
| `--font-path <PATH>` | Extra font file or directory (repeatable) |
| `--no-system-fonts` | Never use installed fonts |
| `--paper <SIZE>` | Paper size such as `a4`, `a5`, `us-letter` (default `a4`) |
| `--margin <LEN>` | Page margin such as `2.5cm`, `20mm`, `1in` (default `2.5cm`) |
| `--toc` | Insert a table of contents after the title |
| `--number-headings` | Number headings as 1, 1.1, 1.1.1 |
| `--code-theme <FILE>` | Syntax highlighting theme (`.tmTheme`) |
| `--template <FILE>` | Custom Typst template replacing the built-in one |
| `--no-cjk-emphasis` | Use strict CommonMark rules for `*` and `_` next to CJK text |
| `--config <FILE>` / `--no-config` | Use another config file / ignore it |

Messages are in Traditional Chinese.

## Settings

Every option can also come from the Markdown front matter or a config file. The precedence
is: command line > front matter > config file > built-in defaults.

**Front matter** sets the title block and PDF metadata, plus any option (keys in kebab-case):

```markdown
---
title: Q3 Report
author: [Alice, Bob]      # a single name works too
date: 2026-09-28
toc: true
paper: a4
---
```

Unknown front matter keys (such as `tags`) are ignored.

**Config file**: `~/.config/mdpdf/config.toml` (or `$XDG_CONFIG_HOME/mdpdf/config.toml`).
Unknown keys are rejected, so typos do not go unnoticed. Relative paths are resolved against
the file's directory.

```toml
paper = "a4"
margin = "2cm"
cjk-font = "LXGW WenKai TC"
font-path = ["~/fonts"]
number-headings = true
system-fonts = true       # allow installed fonts (default)
cjk-emphasis = true
```

## Markdown syntax

Everything in [CommonMark](https://spec.commonmark.org/0.31.2/) works, plus:

| Syntax | Result |
| --- | --- |
| `\| a \| b \|` tables with `:---:` alignment | Tables; the header repeats on every page |
| `~~text~~` or `~text~` | Strikethrough |
| `- [ ]` / `- [x]` | Task list checkboxes |
| `text[^1]` and `[^1]: note` | Footnotes at the bottom of the page |
| `$E = mc^2$` | Inline math (LaTeX) |
| `$$ ... $$` or ```` ```math ```` | Display math |
| `<!-- pagebreak -->` on its own line | Page break (invisible on GitHub and in editors) |

Notes:

- **Math** is converted from LaTeX by [MiTeX](https://github.com/mitex-rs/mitex). `$` follows
  pandoc's rules so prices are not mistaken for math: `$5 and $10` stays text, and `\$` is a
  literal dollar sign. A formula that cannot be converted or typeset is shown as source text
  with a warning.
- **CJK emphasis**: CommonMark does not let `**` close right after punctuation such as `」`
  when a CJK character follows, so `這是**「重點」**這樣` would not be bold. mdpdf relaxes the
  rule next to CJK characters; `--no-cjk-emphasis` restores the strict behavior.
- **Emphasis** (`*text*`) is rendered slanted: the embedded CJK font has no italic style.
- **Raw HTML** is not rendered. Tags of standard HTML elements are dropped and their text is
  kept, `<br>` becomes a line break, and non-HTML "tags" such as `Vec<T>` stay as text.
- **Images** are resolved relative to the Markdown file. Remote images are not downloaded
  yet and become placeholders.
- **Page breaks** only work at the top level, not inside lists, quotes or footnotes.

## Fonts

The binary embeds Noto Sans TC (Regular and Bold) for CJK text, plus Typst's bundled Latin,
math and monospace fonts, so output is identical on every machine. Installed fonts are only
scanned when they are needed: when an option names a font that is not embedded, when the text
contains characters the embedded fonts lack (emoji, other scripts), or when a custom template
is used. `mdpdf fonts` lists every available name.

When a requested font is missing, mdpdf warns and falls back to the default.

## Custom templates

`--template my.typ` replaces the built-in style ([`assets/template.typ`](assets/template.typ)).
The generated document is laid out as:

1. helper functions (`mdpdf-checkbox`, `mdpdf-title`)
2. your template
3. settings from options and front matter (`#set page(...)`, fonts, heading numbering, ...)
4. the title block (`#mdpdf-title(...)`) and table of contents
5. the converted Markdown

Because the template comes after the helpers, it can redefine them, for example
`#let mdpdf-title(title: none, author: (), date: none) = ...` for a custom title block.
Paths in the template (such as `#image("logo.png")`) are relative to the Markdown file.
Use `--emit-typst` to see the full generated source.

## Development

The workspace has three crates:

| Crate | Purpose |
| --- | --- |
| [`mdparse`](crates/mdparse) | CommonMark + GFM parser with no dependencies, and an HTML renderer for the spec tests |
| [`md2typst`](crates/md2typst) | AST → Typst source; all text is emitted as string literals, so Markdown can never inject Typst markup |
| [`mdpdf`](crates/mdpdf) | CLI, in-memory Typst world, fonts, settings, PDF output |

```sh
cargo test --workspace                 # unit, spec, snapshot, CLI and end-to-end tests
SPEC_VERBOSE=1 cargo test -p mdparse --test spec -- --nocapture   # spec results per section
cargo bench -p mdpdf                   # conversion speed on a 30-page report
cargo +nightly fuzz run parse          # fuzzing, see fuzz/Cargo.toml
```

Tests include the CommonMark and GFM spec examples, cmark's pathological inputs, randomized
inputs, and an escaping property test that compiles random text full of Markdown, Typst and
CJK special characters.

Generated files: `scripts/gen-tables.py` (HTML entities and Unicode tables) and
`scripts/extract-gfm-spec.py` (GFM test examples).

## License

mdpdf is licensed under the GNU General Public License v3.0 or later; see [`LICENSE`](LICENSE).
The binary also contains:

- [Typst](https://github.com/typst/typst), Apache-2.0
- [MiTeX](https://github.com/mitex-rs/mitex) and its LaTeX command definitions, Apache-2.0
  ([`assets/typst-packages/mdpdf/mitex-scope`](assets/typst-packages/mdpdf/mitex-scope))
- [Noto Sans TC](https://github.com/notofonts/noto-cjk), SIL Open Font License 1.1
