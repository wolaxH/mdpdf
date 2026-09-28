//! Font loading.
//!
//! Sources are loaded in order; for fonts with the same family and variant, the first one wins:
//! 1. files or directories given with `--font-path`
//! 2. system fonts (disable with `--no-system-fonts`)
//! 3. embedded CJK fonts (feature `embed-cjk`)
//! 4. Latin, math and monospace fonts bundled with Typst

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use typst::foundations::Bytes;
use typst::text::Font;
use typst_kit::fonts::FontStore;

#[derive(Debug, Clone)]
pub struct FontOptions {
    /// Extra font files or directories.
    pub font_paths: Vec<PathBuf>,
    /// Whether to scan system font directories.
    pub system_fonts: bool,
}

impl Default for FontOptions {
    fn default() -> Self {
        Self {
            font_paths: Vec::new(),
            system_fonts: true,
        }
    }
}

pub fn load(options: &FontOptions) -> Result<FontStore> {
    let mut store = FontStore::new();

    for path in &options.font_paths {
        if path.is_dir() {
            store.extend(typst_kit::fonts::scan(path));
        } else {
            let data =
                fs::read(path).with_context(|| format!("無法讀取字型 {}", path.display()))?;
            let fonts: Vec<_> = Font::iter(Bytes::new(data)).map(with_info).collect();
            if fonts.is_empty() {
                bail!("{} 不是可用的字型檔", path.display());
            }
            store.extend(fonts);
        }
    }

    if options.system_fonts {
        store.extend(typst_kit::fonts::system());
    }

    #[cfg(feature = "embed-cjk")]
    store.extend(embedded_cjk());

    store.extend(typst_kit::fonts::embedded());
    Ok(store)
}

fn with_info(font: Font) -> (Font, typst::text::FontInfo) {
    let info = font.info().clone();
    (font, info)
}

#[cfg(feature = "embed-cjk")]
fn embedded_cjk() -> impl Iterator<Item = (Font, typst::text::FontInfo)> {
    mod generated {
        include!(concat!(env!("OUT_DIR"), "/embedded_fonts.rs"));
    }

    generated::CJK_FONTS.iter().flat_map(|compressed| {
        let data = zstd::decode_all(*compressed).expect("embedded fonts are valid zstd data");
        Font::iter(Bytes::new(data)).map(with_info)
    })
}
