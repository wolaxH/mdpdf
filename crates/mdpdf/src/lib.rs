//! mdpdf: Markdown → Typst → PDF.

pub mod fonts;
pub mod settings;
pub mod world;

use std::path::{Path, PathBuf};

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
    /// Maps positions in `source` back to Markdown lines.
    pub source_map: md2typst::SourceMap,
    /// Formulas and images of the last conversion.
    pub fallibles: Vec<md2typst::Fallible>,
    /// Markdown-level warnings (missing images, formulas shown as raw text, ...).
    pub warnings: Vec<md2typst::Warning>,
    /// Typst compilation result and Typst warnings.
    pub result: Warned<SourceResult<Pdf>>,
}

/// Maximum number of retries after a layout failure (each round falls back the failing elements).
const MAX_RETRIES: usize = 10;

/// The complete Markdown → PDF pipeline.
///
/// Some elements only fail when Typst lays them out: a formula MiTeX converted can still fail to
/// evaluate (e.g. `\left(` without a matching `\right`), and an image file can be corrupt. In that
/// case, find the elements the errors fall into, replace them with their fallback (raw source or a
/// placeholder) and compile again, instead of failing the whole document.
pub fn render(
    markdown: &str,
    main_path: &Path,
    mut options: md2typst::Options,
    fonts: FontStore,
) -> Result<Rendered> {
    let mut converted = md2typst::convert(markdown, &options);
    let mut world = MdWorld::new(main_path, converted.source.clone(), fonts)?;
    let mut element_warnings = Vec::new();

    for attempt in 0.. {
        let result = compile_pdf(&world);
        let failed = match &result.output {
            Err(errors) if attempt < MAX_RETRIES => {
                failing_elements(&world, errors, &converted.fallibles)
                    .into_iter()
                    .filter(|(index, _)| !options.fallback.contains(index))
                    .collect()
            }
            _ => Vec::new(),
        };
        if failed.is_empty() {
            let mut warnings = converted.warnings;
            warnings.extend(element_warnings);
            warnings.sort_by_key(|w| w.line);
            return Ok(Rendered {
                world,
                source: converted.source,
                source_map: converted.source_map,
                fallibles: converted.fallibles,
                warnings,
                result,
            });
        }
        for (index, message) in failed {
            let failed_kind = &converted.fallibles[index].kind;
            // Identical formulas or images fail the same way, so fall them all back at once
            // instead of discovering them one compile at a time.
            for (other, item) in converted.fallibles.iter().enumerate() {
                if item.kind != *failed_kind || !options.fallback.insert(other) {
                    continue;
                }
                let message = match &item.kind {
                    md2typst::FallibleKind::Math(tex) => {
                        format!("公式 `{tex}` 無法排版：{message}，改以原文顯示")
                    }
                    md2typst::FallibleKind::Image(url) => {
                        format!("圖片 {url} 無法載入：{message}，改用佔位框")
                    }
                };
                element_warnings.push(md2typst::Warning {
                    line: item.line,
                    message,
                });
            }
        }
        converted = md2typst::convert(markdown, &options);
        world.set_main_text(converted.source.clone());
    }
    unreachable!()
}

impl Rendered {
    /// Local image files referenced by the document, resolved against `base`.
    pub fn image_paths(&self, base: &Path) -> Vec<PathBuf> {
        self.fallibles
            .iter()
            .filter_map(|item| match &item.kind {
                md2typst::FallibleKind::Image(url) if !url.contains("://") => Some(base.join(url)),
                _ => None,
            })
            .collect()
    }

    /// The Markdown line a Typst diagnostic comes from: its own location if that is in the
    /// converted body, otherwise the innermost call site in the body (e.g. when a template
    /// function fails). `None` when it lies entirely in the prelude, template or packages.
    pub fn markdown_line(&self, diagnostic: &SourceDiagnostic) -> Option<u32> {
        let main = typst::World::main(&self.world);
        let spans = std::iter::once(diagnostic.span)
            .chain(diagnostic.trace.iter().map(|t| DiagSpan::from(t.span)));
        spans
            .filter(|span| span.id() == Some(main))
            .filter_map(|span| self.world.range(span))
            .find_map(|range| self.source_map.line_at(range.start))
    }
}

/// Find the elements that error locations (or their call traces) fall into. Returns (element index, error message).
fn failing_elements(
    world: &MdWorld,
    errors: &[SourceDiagnostic],
    elements: &[md2typst::Fallible],
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
            let hit = elements
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
