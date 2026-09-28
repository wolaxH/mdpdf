//! 產生內嵌資源：
//! - `assets/fonts/` 的 CJK 字型，以 zstd 壓縮
//! - `assets/typst-packages/<命名空間>/<名稱>/<版本>/` 下的 Typst 套件

use std::env;
use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};

/// 內嵌的字型檔（位於 `assets/fonts/`）。
const EMBEDDED_CJK: &[&str] = &["NotoSansTC-Regular.otf", "NotoSansTC-Bold.otf"];

fn main() {
    println!("cargo::rerun-if-changed=build.rs");

    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let mut entries = String::new();

    if env::var_os("CARGO_FEATURE_EMBED_CJK").is_some() {
        let dir =
            PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../../assets/fonts");
        for name in EMBEDDED_CJK {
            let src = dir.join(name);
            println!("cargo::rerun-if-changed={}", src.display());
            let data = fs::read(&src).unwrap_or_else(|err| {
                panic!(
                    "無法讀取內嵌字型 {}：{err}\n\
                     請先執行 scripts/fetch-fonts.sh，或以 --no-default-features 建置",
                    src.display()
                )
            });
            let dst = out.join(format!("{name}.zst"));
            fs::write(&dst, zstd::encode_all(&data[..], 19).unwrap()).unwrap();
            writeln!(entries, "    include_bytes!({dst:?}),").unwrap();
        }
    }

    fs::write(
        out.join("embedded_fonts.rs"),
        format!(
            "/// zstd 壓縮後的內嵌 CJK 字型。\npub static CJK_FONTS: &[&[u8]] = &[\n{entries}];\n"
        ),
    )
    .unwrap();

    embed_packages(&out);
}

/// 把 Typst 套件的每個檔案列成 `(套件, 套件內路徑, 內容)` 的表。
fn embed_packages(out: &Path) {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("../../assets/typst-packages");
    println!("cargo::rerun-if-changed={}", root.display());
    let mut files = Vec::new();
    collect_files(&root, &mut files);
    files.sort();

    let mut entries = String::new();
    for path in files {
        println!("cargo::rerun-if-changed={}", path.display());
        let rel = path.strip_prefix(&root).unwrap();
        let mut parts = rel.components().map(|c| c.as_os_str().to_str().unwrap());
        let (ns, name, version) = (
            parts.next().unwrap(),
            parts.next().unwrap(),
            parts.next().unwrap(),
        );
        let inner = parts.collect::<Vec<_>>().join("/");
        let path = path.canonicalize().unwrap();
        writeln!(
            entries,
            "    (\"@{ns}/{name}:{version}\", \"/{inner}\", include_bytes!({path:?})),"
        )
        .unwrap();
    }
    fs::write(
        out.join("embedded_packages.rs"),
        format!("/// 內嵌的 Typst 套件檔案：（套件, 套件內路徑, 內容）。\npub static PACKAGE_FILES: &[(&str, &str, &[u8])] = &[\n{entries}];\n"),
    )
    .unwrap();
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            println!("cargo::rerun-if-changed={}", path.display());
            collect_files(&path, out);
        } else {
            out.push(path);
        }
    }
}
