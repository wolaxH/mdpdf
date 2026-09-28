//! 記憶體中的 Typst [`World`]。
//!
//! 主檔（codegen 產生的 Typst 原始碼）只存在記憶體；其餘檔案（圖片等）以
//! Markdown 所在目錄為根目錄從磁碟讀取。不支援 Typst 套件。
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
    /// 建立 World。
    ///
    /// `root` 是相對路徑的基準目錄，`main_name` 是主檔在錯誤訊息中顯示的名稱。
    pub fn new(
        root: PathBuf,
        main_name: &str,
        main_text: String,
        fonts: FontStore,
    ) -> Result<Self> {
        let vpath = VirtualPath::new(format!("/{main_name}"))
            .map_err(|err| anyhow!("無效的檔名 {main_name:?}：{err}"))?;
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

    /// 相對路徑的基準目錄。
    pub fn root(&self) -> &Path {
        self.files.loader().root.path()
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
        id.vpath().get_without_slash().to_string()
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
