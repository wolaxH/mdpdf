use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::Parser;
use mdpdf::fonts::FontOptions;
use mdpdf::{MdWorld, compile_pdf};
use typst::diag::SourceDiagnostic;
use typst_kit::diagnostics::termcolor::{ColorChoice, StandardStream};
use typst_kit::diagnostics::{DiagnosticFormat, emit};

/// 把 Markdown 轉成 PDF。
#[derive(Debug, Parser)]
#[command(version, about)]
struct Cli {
    /// 輸入的 Markdown 檔，`-` 代表 stdin
    input: PathBuf,

    /// 輸出路徑（預設：輸入檔名.pdf；`-` 代表 stdout）
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// 另存中間產生的 Typst 原始碼
    #[arg(long, value_name = "FILE")]
    emit_typst: Option<PathBuf>,

    /// 額外字型檔或目錄（可重複指定）
    #[arg(long, value_name = "PATH")]
    font_path: Vec<PathBuf>,

    /// 不讀取系統字型
    #[arg(long)]
    no_system_fonts: bool,

    /// 警告視為錯誤（例如找不到圖片）
    #[arg(long)]
    strict: bool,

    /// 關閉 CJK 寬鬆強調，完全依照 CommonMark 規則判斷 `**` 能否成立
    #[arg(long)]
    no_cjk_emphasis: bool,
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("錯誤：{err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode> {
    let stdin = cli.input == Path::new("-");
    let (markdown, dir, name) = if stdin {
        let mut text = String::new();
        io::stdin()
            .read_to_string(&mut text)
            .context("無法讀取 stdin")?;
        (text, std::env::current_dir()?, "stdin".to_string())
    } else {
        let text = fs::read_to_string(&cli.input)
            .with_context(|| format!("無法讀取 {}", cli.input.display()))?;
        let dir = cli
            .input
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let name = cli
            .input
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        (text, dir.to_path_buf(), name)
    };
    let dir = dir
        .canonicalize()
        .with_context(|| format!("無法解析目錄 {}", dir.display()))?;

    let output = match (cli.output, stdin) {
        (Some(path), _) => path,
        (None, false) => cli.input.with_extension("pdf"),
        (None, true) => bail!("從 stdin 讀取時請用 -o 指定輸出路徑"),
    };

    let converted = md2typst::convert(
        &markdown,
        &md2typst::Options {
            base_dir: Some(dir.clone()),
            parse: mdparse::Options {
                cjk_emphasis: !cli.no_cjk_emphasis,
                ..Default::default()
            },
        },
    );
    for warning in &converted.warnings {
        eprintln!("警告：{name}:{}：{}", warning.line, warning.message);
    }
    if cli.strict && !converted.warnings.is_empty() {
        bail!(
            "有 {} 個警告，--strict 模式下停止",
            converted.warnings.len()
        );
    }
    if let Some(path) = &cli.emit_typst {
        fs::write(path, &converted.source)
            .with_context(|| format!("無法寫入 {}", path.display()))?;
    }

    let fonts = mdpdf::fonts::load(&FontOptions {
        font_paths: cli.font_path,
        system_fonts: !cli.no_system_fonts,
    })?;
    let world = MdWorld::new(&dir.join(format!("{name}.typ")), converted.source, fonts)?;

    let result = compile_pdf(&world);
    print_diagnostics(&world, &result.warnings)?;
    let pdf = match result.output {
        Ok(pdf) => pdf,
        Err(errors) => {
            print_diagnostics(&world, &errors)?;
            return Ok(ExitCode::FAILURE);
        }
    };

    if output == Path::new("-") {
        let mut stdout = io::stdout().lock();
        if stdout.is_terminal() {
            bail!("拒絕把 PDF 輸出到終端機，請重新導向或用 -o 指定檔案");
        }
        stdout.write_all(&pdf.bytes)?;
        stdout.flush()?;
    } else {
        fs::write(&output, &pdf.bytes).with_context(|| format!("無法寫入 {}", output.display()))?;
        eprintln!("已輸出 {}（{} 頁）", output.display(), pdf.pages);
    }
    Ok(ExitCode::SUCCESS)
}

fn print_diagnostics(world: &MdWorld, diagnostics: &[SourceDiagnostic]) -> Result<()> {
    if diagnostics.is_empty() {
        return Ok(());
    }
    let color = if io::stderr().is_terminal() {
        ColorChoice::Auto
    } else {
        ColorChoice::Never
    };
    let mut stderr = StandardStream::stderr(color);
    emit(&mut stderr, world, diagnostics, DiagnosticFormat::Human).context("無法輸出診斷訊息")
}
