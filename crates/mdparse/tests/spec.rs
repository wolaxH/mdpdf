//! CommonMark 規格測試：把每個範例的 Markdown 渲染成 HTML 後與預期輸出比對。
//!
//! 設定 `SPEC_VERBOSE=1` 可印出失敗的範例，`SPEC_SECTION=<名稱>` 只看某一節。

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

#[derive(Deserialize)]
struct Example {
    markdown: String,
    html: String,
    example: u32,
    section: String,
}

/// 目前至少要通過的範例數。parser 進步後調高，防止退步。
const MIN_PASSED: usize = 389;

#[test]
fn commonmark_spec() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/spec/commonmark-0.31.2.json");
    let examples: Vec<Example> =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let verbose = std::env::var_os("SPEC_VERBOSE").is_some();
    let only = std::env::var("SPEC_SECTION").ok();

    let mut sections: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    let mut passed = 0;
    for ex in &examples {
        let actual = mdparse::html::render(&mdparse::parse(&ex.markdown));
        let ok = actual == ex.html;
        let entry = sections.entry(&ex.section).or_default();
        entry.1 += 1;
        if ok {
            entry.0 += 1;
            passed += 1;
        } else if verbose && only.as_deref().is_none_or(|s| s == ex.section) {
            eprintln!(
                "--- 範例 {}（{}）\n輸入：{:?}\n預期：{:?}\n實際：{:?}",
                ex.example, ex.section, ex.markdown, ex.html, actual
            );
        }
    }

    for (name, (ok, total)) in &sections {
        eprintln!("{ok:>4}/{total:<4} {name}");
    }
    eprintln!(
        "合計 {passed}/{}（{:.1}%）",
        examples.len(),
        passed as f64 * 100.0 / examples.len() as f64
    );
    assert!(
        passed >= MIN_PASSED,
        "通過數 {passed} 低於門檻 {MIN_PASSED}"
    );
}
