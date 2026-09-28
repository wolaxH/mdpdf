//! 每個 fixture 的 Typst 輸出快照。輸出變更時需以 `cargo insta review` 人工確認。

use std::path::Path;

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name);
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn m0_demo() {
    insta::assert_snapshot!(md2typst::body(&fixture("m0-demo.md"), &Default::default()).source);
}

#[test]
fn m1_blocks() {
    insta::assert_snapshot!(md2typst::body(&fixture("m1-blocks.md"), &Default::default()).source);
}

#[test]
fn m2_inline() {
    insta::assert_snapshot!(md2typst::body(&fixture("m2-inline.md"), &Default::default()).source);
}

#[test]
fn m3_gfm() {
    insta::assert_snapshot!(md2typst::body(&fixture("m3-gfm.md"), &Default::default()).source);
}

#[test]
fn m4_math() {
    insta::assert_snapshot!(md2typst::body(&fixture("m4-math.md"), &Default::default()).source);
}
