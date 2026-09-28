//! mdpdf：Markdown → Typst → PDF。

pub mod fonts;
pub mod world;

use typst::diag::{SourceResult, Warned};
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
