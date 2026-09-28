//! mdpdf: Markdown → Typst → PDF.

pub mod fonts;
pub mod settings;
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

/// A compiled PDF.
pub struct Pdf {
    pub bytes: Vec<u8>,
    pub pages: usize,
    /// Size of the first page in points (width, height).
    pub page_size: (f64, f64),
}

/// Compile the main file of the World and export a PDF.
pub fn compile_pdf(world: &MdWorld) -> Warned<SourceResult<Pdf>> {
    let Warned { output, warnings } = typst::compile::<PagedDocument>(world);
    let output = output.and_then(|doc| {
        let bytes = typst_pdf::pdf(&doc, &PdfOptions::default())?;
        let size = doc.pages().first().map(|page| page.frame.size());
        Ok(Pdf {
            bytes,
            pages: doc.pages().len(),
            page_size: size.map_or((0.0, 0.0), |s| (s.x.to_pt(), s.y.to_pt())),
        })
    });
    Warned { output, warnings }
}

/// Result of [`render`].
pub struct Rendered {
    /// The World used for the last compilation, needed to print Typst diagnostics.
    pub world: MdWorld,
    /// Typst source of the last compilation.
    pub source: String,
    /// Markdown-level warnings (missing images, formulas shown as raw text, ...).
    pub warnings: Vec<md2typst::Warning>,
    /// Typst compilation result and Typst warnings.
    pub result: Warned<SourceResult<Pdf>>,
}

/// Maximum number of retries after a layout failure (each round turns failing formulas into raw text).
const MAX_MATH_RETRIES: usize = 5;

/// The complete Markdown → PDF pipeline.
///
/// A formula that MiTeX converted successfully can still fail when Typst evaluates it (e.g. `\left(`
/// without a matching `\right`). In that case, find the formulas the errors fall into, show them as
/// raw text and compile again, instead of failing the whole document.
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

/// Find the formulas that error locations (or their call traces) fall into. Returns (formula index, error message).
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
