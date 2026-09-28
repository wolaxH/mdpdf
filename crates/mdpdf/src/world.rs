//! 記憶體中的 Typst [`World`]。
//!
//! 主檔（codegen 產生的 Typst 原始碼）只存在記憶體，虛擬路徑放在 Markdown 所在目錄；
//! 其餘檔案（圖片等）從磁碟讀取。專案根目錄設為檔案系統的 `/`，因此 Markdown 裡
//! `../img.png` 或絕對路徑的圖片都能正常解析。不支援 Typst 套件。
//!
//! typst 的 API 在版本間變動頻繁，所有與它直接互動的程式集中在這個模組。

use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use typst::diag::{FileError, FileResult, PackageError};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};
use typst_kit::datetime::Time;
use typst_kit::diagnostics::DiagnosticWorld;
use typst_kit::files::{FileLoader, FileStore, FsRoot};
use typst_kit::fonts::FontStore;

pub struct MdWorld {
    library: LazyHash<Library>,
    fonts: FontStore,
    files: FileStore<Loader>,
    main: FileId,
    time: Time,
}

impl MdWorld {
    /// 建立 World。`main_path` 是主檔的絕對路徑（不必真的存在），
    /// 決定相對路徑的基準目錄與錯誤訊息中顯示的名稱。
    pub fn new(main_path: &Path, main_text: String, fonts: FontStore) -> Result<Self> {
        let root = PathBuf::from("/");
        let vpath = VirtualPath::virtualize(&root, main_path)
            .map_err(|err| anyhow!("無效的路徑 {}：{err}", main_path.display()))?;
        let main = RootedPath::new(VirtualRoot::Project, vpath).intern();
        let loader = Loader {
            root: FsRoot::new(root),
            main,
            main_text: Bytes::from_string(main_text),
        };

        Ok(Self {
            library: LazyHash::new(Library::builder().build()),
            fonts,
            files: FileStore::new(loader),
            main,
            time: reproducible_time(),
        })
    }
}

impl World for MdWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        self.fonts.book()
    }

    fn main(&self) -> FileId {
        self.main
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        self.files.source(id)
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        self.files.file(id)
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.font(index)
    }

    fn today(&self, offset: Option<Duration>) -> Option<Datetime> {
        self.time.today(offset)
    }
}

impl DiagnosticWorld for MdWorld {
    fn name(&self, id: FileId) -> String {
        id.vpath().get_with_slash().to_string()
    }
}

struct Loader {
    root: FsRoot,
    main: FileId,
    main_text: Bytes,
}

impl FileLoader for Loader {
    fn load(&self, id: FileId) -> FileResult<Bytes> {
        if id == self.main {
            return Ok(self.main_text.clone());
        }
        match id.root() {
            VirtualRoot::Project => self.root.load(id.vpath()),
            VirtualRoot::Package(spec) => {
                Err(FileError::Package(PackageError::NotFound(spec.clone())))
            }
        }
    }
}

/// 設定 `SOURCE_DATE_EPOCH` 時使用固定時間，讓輸出可重現。
fn reproducible_time() -> Time {
    std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|s| s.parse().ok())
        .and_then(|ts| Time::fixed_timestamp(ts).ok())
        .unwrap_or_else(Time::system)
}
