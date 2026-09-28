//! 把 `assets/fonts/` 裡的 CJK 字型以 zstd 壓縮後內嵌進 binary。

use std::env;
use std::fmt::Write;
use std::fs;
use std::path::PathBuf;

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
}
