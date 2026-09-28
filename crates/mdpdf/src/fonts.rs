//! 字型載入。
//!
//! 依序載入，同名同字重的字型以先載入者為準：
//! 1. `--font-path` 指定的檔案或目錄
//! 2. 系統字型（可用 `--no-system-fonts` 關閉）
//! 3. 內嵌 CJK 字型（feature `embed-cjk`）
//! 4. Typst 附帶的拉丁、數學、等寬字型

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use typst::foundations::Bytes;
use typst::text::Font;
use typst_kit::fonts::FontStore;

#[derive(Debug, Clone)]
pub struct FontOptions {
    /// 額外的字型檔或目錄。
    pub font_paths: Vec<PathBuf>,
    /// 是否掃描系統字型目錄。
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
        let data = zstd::decode_all(*compressed).expect("內嵌字型應為有效的 zstd 資料");
        Font::iter(Bytes::new(data)).map(with_info)
    })
}
