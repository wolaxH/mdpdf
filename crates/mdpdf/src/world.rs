//! In-memory Typst [`World`].
//!
//! The main file (Typst source produced by codegen) exists only in memory, with a virtual path in
//! the Markdown file's directory; other files (images, ...) are read from disk. The project root is
//! the file system root `/`, so images at `../img.png` or absolute paths resolve normally. Typst packages are served only from the embedded set.
//!
//! The typst API changes often between versions, so all code that talks to it directly lives in this module.

use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use typst::diag::{FileError, FileResult, PackageError};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::syntax::package::PackageSpec;
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
    /// Create a World. `main_path` is the absolute path of the main file (it need not exist); it
    /// determines the base directory for relative paths and the name shown in diagnostics.
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

impl MdWorld {
    /// Replace the main file content (e.g. source regenerated after formulas fell back to raw text); other file caches are cleared too.
    pub fn set_main_text(&mut self, text: String) {
        let loader = self.files.loader_mut();
        loader.main_text = Bytes::from_string(text);
        self.files.reset();
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
            VirtualRoot::Package(spec) => embedded_package_file(spec, id.vpath().get_with_slash())
                .ok_or_else(|| FileError::Package(PackageError::NotFound(spec.clone()))),
        }
    }
}

mod generated {
    include!(concat!(env!("OUT_DIR"), "/embedded_packages.rs"));
}

/// Embedded Typst packages (see `assets/typst-packages/`). Packages are never downloaded.
fn embedded_package_file(spec: &PackageSpec, path: &str) -> Option<Bytes> {
    let spec = spec.to_string();
    generated::PACKAGE_FILES
        .iter()
        .find(|(pkg, file, _)| *pkg == spec && *file == path)
        .map(|(_, _, data)| Bytes::new(*data))
}

/// Use a fixed time when `SOURCE_DATE_EPOCH` is set, for reproducible output.
fn reproducible_time() -> Time {
    std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|s| s.parse().ok())
        .and_then(|ts| Time::fixed_timestamp(ts).ok())
        .unwrap_or_else(Time::system)
}
