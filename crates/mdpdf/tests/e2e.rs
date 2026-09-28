//! 端到端：fixture → Typst → PDF。

use std::path::Path;

use mdpdf::fonts::{self, FontOptions};
use mdpdf::{MdWorld, compile_pdf};

fn render(name: &str) -> mdpdf::Pdf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    let markdown = std::fs::read_to_string(dir.join(name)).unwrap();
    // 不讀系統字型，確保結果只取決於內嵌字型。
    let fonts = fonts::load(&FontOptions {
        system_fonts: false,
        ..Default::default()
    })
    .unwrap();
    let dir = dir.canonicalize().unwrap();
    let options = md2typst::Options {
        base_dir: Some(dir.clone()),
        ..Default::default()
    };
    let converted = md2typst::convert(&markdown, &options);
    assert!(
        converted.warnings.is_empty(),
        "codegen 警告：{:#?}",
        converted.warnings
    );
    let world = MdWorld::new(&dir.join("main.typ"), converted.source, fonts).unwrap();

    let result = compile_pdf(&world);
    let pdf = result
        .output
        .unwrap_or_else(|errors| panic!("編譯失敗：{errors:#?}"));
    // 找不到字型時 typst 只會發警告，因此警告也視為失敗。
    assert!(
        result.warnings.is_empty(),
        "出現警告：{:#?}",
        result.warnings
    );
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
