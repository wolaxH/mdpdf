//! 規格測試：把每個範例的 Markdown 渲染成 HTML 後與預期輸出比對。
//!
//! 設定 `SPEC_VERBOSE=1` 可印出失敗的範例，`SPEC_SECTION=<名稱>` 只看某一節。

use std::collections::BTreeMap;
use std::path::Path;

use mdparse::Options;
use serde::Deserialize;

#[derive(Deserialize)]
struct Example {
    markdown: String,
    html: String,
    section: String,
}

struct Report {
    passed: usize,
    total: usize,
}

fn run(file: &str, options: Options, normalize: fn(&str) -> String) -> Report {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/spec")
        .join(file);
    let examples: Vec<Example> =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let verbose = std::env::var_os("SPEC_VERBOSE").is_some();
    let only = std::env::var("SPEC_SECTION").ok();

    let mut sections: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    let mut passed = 0;
    for (i, ex) in examples.iter().enumerate() {
        let actual = mdparse::html::render(&mdparse::parse_with(&ex.markdown, options));
        let ok = normalize(&actual) == normalize(&ex.html);
        let entry = sections.entry(&ex.section).or_default();
        entry.1 += 1;
        if ok {
            entry.0 += 1;
            passed += 1;
        } else if verbose && only.as_deref().is_none_or(|s| s == ex.section) {
            eprintln!(
                "--- {file} #{}（{}）\n輸入：{:?}\n預期：{:?}\n實際：{:?}",
                i + 1,
                ex.section,
                ex.markdown,
                ex.html,
                actual
            );
        }
    }

    eprintln!("== {file}（{options:?}）");
    for (name, (ok, total)) in &sections {
        eprintln!("{ok:>4}/{total:<4} {name}");
    }
    let total = examples.len();
    eprintln!(
        "合計 {passed}/{total}（{:.1}%）",
        passed as f64 * 100.0 / total as f64
    );
    Report { passed, total }
}

fn identity(html: &str) -> String {
    html.to_string()
}

/// cmark-gfm 不同版本的任務清單 `<input>` 屬性順序不同，比對前統一格式。
fn normalize_checkbox(html: &str) -> String {
    html.replace(
        "<input type=\"checkbox\" checked=\"\" disabled=\"\" />",
        "<input checked=\"\" disabled=\"\" type=\"checkbox\">",
    )
    .replace(
        "<input type=\"checkbox\" disabled=\"\" />",
        "<input disabled=\"\" type=\"checkbox\">",
    )
}

#[test]
fn commonmark_spec() {
    let report = run("commonmark-0.31.2.json", Options::commonmark(), identity);
    assert_eq!(report.passed, report.total, "CommonMark 規格必須全數通過");
}

/// 開啟所有擴充時，一般 CommonMark 文件的解析結果仍應幾乎不變。
#[test]
fn commonmark_spec_with_extensions() {
    let report = run("commonmark-0.31.2.json", Options::default(), identity);
    assert_eq!(
        report.passed, report.total,
        "開啟擴充不應改變 CommonMark 範例的結果"
    );
}

#[test]
fn gfm_extensions() {
    let report = run(
        "gfm-extensions.json",
        Options::default(),
        normalize_checkbox,
    );
    assert_eq!(report.passed, report.total, "GFM 擴充範例必須全數通過");
}
