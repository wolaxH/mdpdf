//! Conversion speed on a ~30-page report built from the test fixtures (headings, lists, tables,
//! code, footnotes, math and images). The goal is well under one second for the whole pipeline.
//!
//! Run with `cargo bench -p mdpdf`.

use std::path::{Path, PathBuf};

use criterion::{Criterion, criterion_group, criterion_main};
use mdpdf::fonts::{self, FontOptions};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .canonicalize()
        .unwrap()
}

/// About 30 pages: the feature fixtures repeated, with footnote labels made unique per copy.
fn report() -> String {
    let dir = fixtures();
    let parts: Vec<String> = ["m1-blocks.md", "m2-inline.md", "m3-gfm.md", "m4-math.md"]
        .iter()
        .map(|name| std::fs::read_to_string(dir.join(name)).unwrap())
        .collect();
    (0..10)
        .map(|i| parts.join("\n\n").replace("[^", &format!("[^c{i}-")))
        .collect::<Vec<_>>()
        .join("\n\n<!-- pagebreak -->\n\n")
}

fn embedded_fonts() -> typst_kit::fonts::FontStore {
    fonts::load(&FontOptions {
        system_fonts: false,
        ..Default::default()
    })
    .unwrap()
}

fn bench(c: &mut Criterion) {
    let markdown = report();
    let dir = fixtures();
    let options = md2typst::Options {
        base_dir: Some(dir.clone()),
        ..Default::default()
    };

    let pages = mdpdf::render(
        &markdown,
        &dir.join("main.typ"),
        options.clone(),
        embedded_fonts(),
    )
    .unwrap()
    .result
    .output
    .unwrap()
    .pages;
    eprintln!(
        "benchmark document: {} bytes, {pages} pages",
        markdown.len()
    );

    let mut group = c.benchmark_group("convert");
    group.sample_size(10);
    group.bench_function("parse", |b| b.iter(|| mdparse::parse(&markdown)));
    group.bench_function("codegen", |b| {
        b.iter(|| md2typst::convert(&markdown, &options))
    });
    group.bench_function("load embedded fonts", |b| b.iter(embedded_fonts));
    // Typst memoizes across compilations in the same process. Clearing the cache on every
    // iteration measures a cold run, like one CLI invocation; keeping it measures a rebuild in
    // watch mode.
    for (name, cold) in [
        ("full pipeline (cold)", true),
        ("full pipeline (warm, as in --watch)", false),
    ] {
        group.bench_function(name, |b| {
            b.iter(|| {
                if cold {
                    typst::comemo::evict(0);
                }
                let rendered = mdpdf::render(
                    &markdown,
                    &dir.join("main.typ"),
                    options.clone(),
                    embedded_fonts(),
                )
                .unwrap();
                assert!(rendered.result.output.is_ok());
            })
        });
    }
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
