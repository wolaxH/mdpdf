//! Command-line tests: run the built binary and check the generated Typst source and PDF.
//!
//! Every test passes `--no-system-fonts` and `--no-config`, so results depend only on embedded
//! fonts and never on the machine's own config file.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Run {
    output: Output,
    typst: String,
    pdf: Option<Vec<u8>>,
}

impl Run {
    fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.output.stderr).into_owned()
    }

    fn success(&self) -> bool {
        self.output.status.success()
    }
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .canonicalize()
        .unwrap()
}

/// A scratch directory per test, removed and recreated on each run.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mdpdf-cli-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Convert `markdown` with extra `args`, emitting the Typst source next to the PDF.
fn run(name: &str, markdown: &str, args: &[&str]) -> Run {
    let dir = scratch(name);
    let input = dir.join("doc.md");
    fs::write(&input, markdown).unwrap();
    let pdf = dir.join("doc.pdf");
    let typ = dir.join("doc.typ");
    let output = Command::new(env!("CARGO_BIN_EXE_mdpdf"))
        .arg(&input)
        .arg("-o")
        .arg(&pdf)
        .arg("--emit-typst")
        .arg(&typ)
        .args(["--no-system-fonts", "--no-config"])
        .args(args)
        .output()
        .unwrap();
    Run {
        output,
        typst: fs::read_to_string(&typ).unwrap_or_default(),
        pdf: fs::read(&pdf).ok(),
    }
}

#[test]
fn default_conversion() {
    let r = run("default", "# 標題\n\n內文\n", &[]);
    assert!(r.success(), "{}", r.stderr());
    assert!(r.pdf.as_ref().unwrap().starts_with(b"%PDF-"));
    assert!(r.stderr().contains("（1 頁）"));
}

#[test]
fn paper_and_margin() {
    let r = run("paper", "x\n", &["--paper", "a5", "--margin", "1.5cm"]);
    assert!(r.success(), "{}", r.stderr());
    assert!(r.typst.contains("#set page(paper: \"a5\")"));
    assert!(r.typst.contains("#set page(margin: 1.5cm)"));

    let bad = run("paper-bad", "x\n", &["--paper", "a99"]);
    assert!(!bad.success());
    assert!(bad.stderr().contains("未知的紙張大小"), "{}", bad.stderr());
    let bad = run("margin-bad", "x\n", &["--margin", "2cm) #panic("]);
    assert!(!bad.success());
    assert!(bad.stderr().contains("無效的邊距"), "{}", bad.stderr());
}

#[test]
fn toc_and_numbered_headings() {
    let r = run("toc", "# 一\n\n## 二\n", &["--toc", "--number-headings"]);
    assert!(r.success(), "{}", r.stderr());
    assert!(r.typst.contains("#outline()"));
    assert!(r.typst.contains("#set heading(numbering: \"1.1\")"));
    let plain = run("no-toc", "# 一\n", &[]);
    assert!(!plain.typst.contains("#outline()") && !plain.typst.contains("numbering: \"1.1\""));
}

#[test]
fn fonts() {
    let r = run(
        "fonts",
        "text `code`\n",
        &[
            "--font",
            "Libertinus Serif",
            "--cjk-font",
            "Noto Sans TC",
            "--mono-font",
            "DejaVu Sans Mono",
        ],
    );
    assert!(r.success(), "{}", r.stderr());
    assert!(
        r.typst
            .contains("#set text(font: (\"Libertinus Serif\", \"Noto Sans TC\", ))")
    );
    assert!(
        r.typst
            .contains("#show raw: set text(font: (\"DejaVu Sans Mono\", \"Noto Sans TC\", ))")
    );

    // A missing font falls back with a warning, and fails under --strict
    let r = run("font-missing", "x\n", &["--font", "No Such Font"]);
    assert!(r.success(), "{}", r.stderr());
    assert!(
        r.stderr().contains("找不到字型「No Such Font」"),
        "{}",
        r.stderr()
    );
    assert!(!r.typst.contains("No Such Font"));
    let strict = run(
        "font-strict",
        "x\n",
        &["--font", "No Such Font", "--strict"],
    );
    assert!(!strict.success());
}

#[test]
fn font_path_and_fonts_subcommand() {
    let fonts_dir = fixtures().join("../../assets/fonts");
    let out = Command::new(env!("CARGO_BIN_EXE_mdpdf"))
        .args(["fonts", "--no-system-fonts", "--no-config"])
        .output()
        .unwrap();
    let families = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    for family in [
        "Noto Sans TC",
        "Libertinus Serif",
        "DejaVu Sans Mono",
        "New Computer Modern Math",
    ] {
        assert!(
            families.lines().any(|l| l == family),
            "{family} missing from:\n{families}"
        );
    }

    // --font-path loads extra fonts; the directory here is the embedded fonts' source
    let r = run(
        "font-path",
        "x\n",
        &[
            "--font-path",
            fonts_dir.to_str().unwrap(),
            "--cjk-font",
            "Noto Sans TC",
        ],
    );
    assert!(r.success(), "{}", r.stderr());
    let bad = run(
        "font-path-bad",
        "x\n",
        &["--font-path", "/no/such/font.otf"],
    );
    assert!(!bad.success());
}

#[test]
fn code_theme_and_template() {
    let theme = fixtures().join("themes/test.tmTheme");
    let r = run(
        "theme",
        "```rust\nfn main() {}\n```\n",
        &["--code-theme", theme.to_str().unwrap()],
    );
    assert!(r.success(), "{}", r.stderr());
    assert!(
        r.typst
            .contains(&format!("#set raw(theme: \"{}\")", theme.display()))
    );
    let bad = run("theme-bad", "x\n", &["--code-theme", "/no/such.tmTheme"]);
    assert!(!bad.success());

    let template = fixtures().join("templates/custom.typ");
    let r = run(
        "template",
        "---\ntitle: T\n---\n\nx\n",
        &["--template", template.to_str().unwrap()],
    );
    assert!(r.success(), "{}", r.stderr());
    assert!(r.typst.contains("A minimal custom template"));
    assert!(!r.typst.contains("mdpdf default style template"));
}

#[test]
fn front_matter_and_precedence() {
    let md =
        "---\ntitle: 報告\nauthor: [甲, 乙]\ndate: 2026-09-28\npaper: a5\ntoc: true\n---\n\n# 一\n";
    let r = run("front", md, &[]);
    assert!(r.success(), "{}", r.stderr());
    assert!(r.typst.contains(
        "#mdpdf-title(title: \"報告\", author: (\"甲\", \"乙\", ), date: \"2026-09-28\")"
    ));
    assert!(r.typst.contains("#set page(paper: \"a5\")"));
    assert!(r.typst.contains("#outline()"));

    // The command line wins over the front matter
    let r = run("front-cli", md, &["--paper", "us-letter"]);
    assert!(r.typst.contains("#set page(paper: \"us-letter\")") && !r.typst.contains("\"a5\""));

    // Malformed front matter is ignored with a warning
    let r = run("front-bad", "---\ntitle: [x\n---\n\ntext\n", &[]);
    assert!(r.success(), "{}", r.stderr());
    assert!(r.stderr().contains("front matter"), "{}", r.stderr());
}

#[test]
fn config_file() {
    let dir = scratch("config-file");
    let config = dir.join("config.toml");
    fs::write(&config, "paper = \"a5\"\nnumber-headings = true\n").unwrap();
    let md = "---\npaper: us-letter\n---\n\n# 一\n";

    // Replace --no-config with --config by running the binary directly
    let input = dir.join("doc.md");
    let typ = dir.join("doc.typ");
    fs::write(&input, md).unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_mdpdf"))
        .arg(&input)
        .args(["--no-system-fonts", "--config"])
        .arg(&config)
        .arg("--emit-typst")
        .arg(&typ)
        .status()
        .unwrap();
    assert!(status.success());
    let typst = fs::read_to_string(&typ).unwrap();
    // The front matter wins over the config file; unset keys come from the config
    assert!(typst.contains("#set page(paper: \"us-letter\")"));
    assert!(typst.contains("#set heading(numbering: \"1.1\")"));

    fs::write(&config, "papr = \"a5\"\n").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_mdpdf"))
        .arg(&input)
        .args(["--no-system-fonts", "--config"])
        .arg(&config)
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("papr"));
}

#[test]
fn cjk_emphasis_toggle() {
    let md = "這是**「重點」**這樣\n";
    assert!(run("cjk-on", md, &[]).typst.contains("#strong["));
    assert!(
        !run("cjk-off", md, &["--no-cjk-emphasis"])
            .typst
            .contains("#strong[")
    );
}

#[test]
fn strict_mode_and_stdin() {
    let r = run("strict", "![x](missing.png)\n", &["--strict"]);
    assert!(!r.success());
    assert!(r.stderr().contains("--strict"), "{}", r.stderr());

    let dir = scratch("stdin");
    let pdf = dir.join("out.pdf");
    let mut child = Command::new(env!("CARGO_BIN_EXE_mdpdf"))
        .args(["-", "--no-system-fonts", "--no-config", "-o"])
        .arg(&pdf)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all("# 來自 stdin\n".as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success());
    assert!(fs::read(&pdf).unwrap().starts_with(b"%PDF-"));

    // stdin without -o is an error
    let out = Command::new(env!("CARGO_BIN_EXE_mdpdf"))
        .args(["-", "--no-config"])
        .output()
        .unwrap();
    assert!(!out.status.success());
}
