//! mdpdf：Markdown → Typst → PDF。

pub mod fonts;
pub mod world;

use std::path::Path;

use anyhow::Result;
use typst::WorldExt;
use typst::diag::{SourceDiagnostic, SourceResult, Warned};
use typst::syntax::DiagSpan;
use typst_kit::fonts::FontStore;
use typst_layout::PagedDocument;
use typst_pdf::PdfOptions;

pub use world::MdWorld;

/// 編譯產出的 PDF。
pub struct Pdf {
    pub bytes: Vec<u8>,
    pub pages: usize,
}

/// 編譯 World 的主檔並輸出 PDF。
pub fn compile_pdf(world: &MdWorld) -> Warned<SourceResult<Pdf>> {
    let Warned { output, warnings } = typst::compile::<PagedDocument>(world);
    let output = output.and_then(|doc| {
        let bytes = typst_pdf::pdf(&doc, &PdfOptions::default())?;
        Ok(Pdf {
            bytes,
            pages: doc.pages().len(),
        })
    });
    Warned { output, warnings }
}

/// [`render`] 的結果。
pub struct Rendered {
    /// 最後一次編譯用的 World，印出 Typst 診斷訊息時需要。
    pub world: MdWorld,
    /// 最後一次編譯的 Typst 原始碼。
    pub source: String,
    /// Markdown 層級的警告（找不到圖片、公式退回原文等）。
    pub warnings: Vec<md2typst::Warning>,
    /// Typst 編譯結果與 Typst 的警告。
    pub result: Warned<SourceResult<Pdf>>,
}

/// 排版失敗時最多重試幾輪（每輪把出錯的公式改為原文）。
const MAX_MATH_RETRIES: usize = 5;

/// Markdown → PDF 的完整流程。
///
/// MiTeX 轉換成功的公式仍可能在 Typst 求值時出錯（例如 `\left(` 沒有對應的 `\right`）。
/// 這時找出錯誤落在哪些公式裡，把它們改成以原文顯示後重新編譯，
/// 而不是讓整份文件失敗。
pub fn render(
    markdown: &str,
    main_path: &Path,
    mut options: md2typst::Options,
    fonts: FontStore,
) -> Result<Rendered> {
    let mut converted = md2typst::convert(markdown, &options);
    let mut world = MdWorld::new(main_path, converted.source.clone(), fonts)?;
    let mut math_warnings = Vec::new();

    for attempt in 0.. {
        let result = compile_pdf(&world);
        let failed = match &result.output {
            Err(errors) if attempt < MAX_MATH_RETRIES => {
                failing_formulas(&world, errors, &converted.formulas)
                    .into_iter()
                    .filter(|(index, _)| !options.math_fallback.contains(index))
                    .collect()
            }
            _ => Vec::new(),
        };
        if failed.is_empty() {
            let mut warnings = converted.warnings;
            warnings.extend(math_warnings);
            warnings.sort_by_key(|w| w.line);
            return Ok(Rendered {
                world,
                source: converted.source,
                warnings,
                result,
            });
        }
        for (index, message) in failed {
            let formula = &converted.formulas[index];
            math_warnings.push(md2typst::Warning {
                line: formula.line,
                message: format!("公式 `{}` 無法排版：{message}，改以原文顯示", formula.tex),
            });
            options.math_fallback.insert(index);
        }
        converted = md2typst::convert(markdown, &options);
        world.set_main_text(converted.source.clone());
    }
    unreachable!()
}

/// 找出錯誤位置（或其呼叫追蹤）落在哪些公式裡。回傳（公式索引, 錯誤訊息）。
fn failing_formulas(
    world: &MdWorld,
    errors: &[SourceDiagnostic],
    formulas: &[md2typst::Formula],
) -> Vec<(usize, String)> {
    let main = typst::World::main(world);
    let mut failed: Vec<(usize, String)> = Vec::new();
    for error in errors {
        let spans =
            std::iter::once(error.span).chain(error.trace.iter().map(|t| DiagSpan::from(t.span)));
        for span in spans {
            if span.id() != Some(main) {
                continue;
            }
            let Some(range) = world.range(span) else {
                continue;
            };
            let hit = formulas
                .iter()
                .position(|f| f.range.start <= range.start && range.end <= f.range.end);
            if let Some(index) = hit
                && !failed.iter().any(|(i, _)| *i == index)
            {
                failed.push((index, error.message.to_string()));
                break;
            }
        }
    }
    failed
}
