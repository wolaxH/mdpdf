//! End to end: fixture → Typst → PDF.

use std::path::Path;

use mdpdf::fonts::{self, FontOptions};

/// Convert a fixture and require it to compile. Returns the PDF and Markdown-level warnings.
fn render_with_warnings(name: &str) -> (mdpdf::Pdf, Vec<md2typst::Warning>) {
    render_with(name, |_| {})
}

/// Like [`render_with_warnings`], with a hook to adjust the codegen options.
fn render_with(
    name: &str,
    adjust: impl FnOnce(&mut md2typst::Options),
) -> (mdpdf::Pdf, Vec<md2typst::Warning>) {
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
    let mut options = md2typst::Options {
        base_dir: Some(dir.clone()),
        ..Default::default()
    };
    adjust(&mut options);
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

fn assert_size(pdf: &mdpdf::Pdf, expected: (f64, f64)) {
    let (w, h) = pdf.page_size;
    assert!(
        (w - expected.0).abs() < 0.1 && (h - expected.1).abs() < 0.1,
        "expected {expected:?}, got ({w}, {h})"
    );
}

const A4: (f64, f64) = (595.28, 841.89);
const A5: (f64, f64) = (419.53, 595.28);

#[test]
#[cfg(feature = "embed-cjk")]
fn page_size_follows_settings() {
    assert_size(&render("m0-demo.md"), A4);

    let (pdf, _) = render_with("m0-demo.md", |o| o.style.paper = Some("a5".into()));
    assert_size(&pdf, A5);

    let template = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/templates/custom.typ"),
    )
    .unwrap();
    let (pdf, _) = render_with("m0-demo.md", |o| o.template = Some(template));
    assert_size(&pdf, A5);
}

#[test]
#[cfg(feature = "embed-cjk")]
fn m5_front_matter_title_and_toc() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    let markdown = std::fs::read_to_string(dir.join("m5-settings.md")).unwrap();
    let yaml = mdparse::front_matter(&markdown).unwrap();
    let (meta, settings) = mdpdf::settings::parse_front_matter(yaml, &dir).unwrap();
    let book = fonts::load(&FontOptions {
        system_fonts: false,
        ..Default::default()
    })
    .unwrap();
    let resolved = mdpdf::settings::resolve(&settings, meta, book.book()).unwrap();
    assert!(resolved.style.toc && resolved.style.number_headings);
    let (pdf, warnings) = render_with("m5-settings.md", |o| o.style = resolved.style);
    assert!(warnings.is_empty(), "{warnings:#?}");
    assert_eq!(pdf.pages, 1);
}

#[test]
#[cfg(feature = "embed-cjk")]
fn many_identical_bad_formulas_all_fall_back() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .canonicalize()
        .unwrap();
    let markdown = "$\\left( x$\n\n".repeat(30);
    let fonts = fonts::load(&FontOptions {
        system_fonts: false,
        ..Default::default()
    })
    .unwrap();
    let rendered =
        mdpdf::render(&markdown, &dir.join("main.typ"), Default::default(), fonts).unwrap();
    assert!(
        rendered.result.output.is_ok(),
        "{:#?}",
        rendered.result.output.err()
    );
    assert_eq!(rendered.warnings.len(), 30);
}
