//! Font loading.
//!
//! Sources are loaded in order; for fonts with the same family and variant, the first one wins:
//! 1. files or directories given with `--font-path`
//! 2. embedded CJK fonts (feature `embed-cjk`)
//! 3. Latin, math and monospace fonts bundled with Typst
//! 4. system fonts, last so that embedded fonts keep the output reproducible

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use typst::foundations::Bytes;
use typst::text::{Font, FontBook, FontInfo};
use typst_kit::fonts::FontStore;

#[derive(Debug, Clone)]
pub struct FontOptions {
    /// Extra font files or directories.
    pub font_paths: Vec<PathBuf>,
    /// Whether to scan system font directories right away.
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

    #[cfg(feature = "embed-cjk")]
    store.extend(embedded_cjk());

    store.extend(typst_kit::fonts::embedded());

    if options.system_fonts {
        add_system_fonts(&mut store);
    }
    Ok(store)
}

/// Scan the system font directories. This reads every installed font file, which can take
/// seconds when the disk cache is cold, so the CLI only does it when [`needs_system_fonts`].
pub fn add_system_fonts(store: &mut FontStore) {
    store.extend(typst_kit::fonts::system());
}

/// Whether the loaded fonts fall short: a requested family is missing, or `text` contains a
/// character that no loaded font covers (emoji, rare scripts, ...).
pub fn needs_system_fonts(book: &FontBook, families: &[&str], text: &str) -> bool {
    if families
        .iter()
        .any(|f| !book.contains_family(&f.to_lowercase()))
    {
        return true;
    }
    let infos: Vec<&FontInfo> = (0..).map_while(|i| book.info(i)).collect();
    let chars: BTreeSet<char> = text
        .chars()
        .filter(|c| !c.is_whitespace() && !c.is_control())
        .collect();
    chars
        .into_iter()
        .any(|c| !infos.iter().any(|info| info.coverage.contains(c as u32)))
}

fn with_info(font: Font) -> (Font, FontInfo) {
    let info = font.info().clone();
    (font, info)
}

#[cfg(feature = "embed-cjk")]
fn embedded_cjk() -> impl Iterator<Item = (Font, FontInfo)> {
    mod generated {
        include!(concat!(env!("OUT_DIR"), "/embedded_fonts.rs"));
    }

    generated::CJK_FONTS.iter().flat_map(|compressed| {
        let data = zstd::decode_all(*compressed).expect("embedded fonts are valid zstd data");
        Font::iter(Bytes::new(data)).map(with_info)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(feature = "embed-cjk")]
    fn system_fonts_only_when_needed() {
        let store = load(&FontOptions {
            system_fonts: false,
            ..Default::default()
        })
        .unwrap();
        let book = store.book();
        assert!(!needs_system_fonts(
            book,
            &[],
            "中英混排 English 「標點」，數學 ∑ `code`"
        ));
        assert!(!needs_system_fonts(
            book,
            &["Noto Sans TC", "libertinus serif"],
            ""
        ));
        assert!(needs_system_fonts(book, &["Some Installed Font"], ""));
        assert!(needs_system_fonts(book, &[], "emoji 😀"));
    }
}
