//! End to end: fixture → Typst → PDF.

use std::path::Path;

use mdpdf::fonts::{self, FontOptions};

/// Convert a fixture and require it to compile. Returns the PDF and Markdown-level warnings.
fn render_with_warnings(name: &str) -> (mdpdf::Pdf, Vec<md2typst::Warning>) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .canonicalize()
        .unwrap();
    let markdown = std::fs::read_to_string(dir.join(name)).unwrap();
    // Skip system fonts so the result depends only on embedded fonts.
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
    let pdf = rendered
        .result
        .output
        .unwrap_or_else(|errors| panic!("編譯失敗：{errors:#?}"));
    // Typst only warns when a font is missing, so Typst warnings count as failures too.
    assert!(
        rendered.result.warnings.is_empty(),
        "出現警告：{:#?}",
        rendered.result.warnings
    );
    (pdf, rendered.warnings)
}

/// Convert a fixture and require no warnings at all.
fn render(name: &str) -> mdpdf::Pdf {
    let (pdf, warnings) = render_with_warnings(name);
    assert!(warnings.is_empty(), "Markdown 警告：{warnings:#?}");
    pdf
}

#[test]
#[cfg(feature = "embed-cjk")]
fn m0_demo_renders_with_embedded_cjk_font() {
    let pdf = render("m0-demo.md");
    assert!(pdf.bytes.starts_with(b"%PDF-"));
    assert_eq!(pdf.pages, 1);
    let needle = b"NotoSansTC";
    assert!(
        pdf.bytes.windows(needle.len()).any(|w| w == needle),
        "PDF 未嵌入 Noto Sans TC"
    );
}

#[test]
#[cfg(feature = "embed-cjk")]
fn m1_blocks_renders() {
    let pdf = render("m1-blocks.md");
    assert!(pdf.pages >= 1);
}

#[test]
#[cfg(feature = "embed-cjk")]
fn m2_inline_renders_with_images() {
    let pdf = render("m2-inline.md");
    assert!(pdf.pages >= 1);
}

#[test]
#[cfg(feature = "embed-cjk")]
fn m3_gfm_renders() {
    let pdf = render("m3-gfm.md");
    assert!(pdf.pages >= 1);
}

#[test]
#[cfg(feature = "embed-cjk")]
fn m4_math_falls_back_on_bad_formulas() {
    let (pdf, warnings) = render_with_warnings("m4-math.md");
    assert!(pdf.pages >= 1);
    // One fails in MiTeX conversion, one in Typst layout; every other formula must succeed
    let messages: Vec<_> = warnings
        .iter()
        .map(|w| (w.line, w.message.as_str()))
        .collect();
    assert_eq!(messages.len(), 2, "{messages:#?}");
    assert!(
        messages[0].1.starts_with("無法轉換公式 `\\foo{x}`"),
        "{messages:#?}"
    );
    assert!(
        messages[1].1.starts_with("公式 `\\left( x` 無法排版"),
        "{messages:#?}"
    );
}

#[test]
#[cfg(feature = "embed-cjk")]
fn page_break_starts_new_page() {
    let pdf = render("pagebreak.md");
    assert_eq!(pdf.pages, 3);
}
