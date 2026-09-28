use std::collections::BTreeSet;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use mdpdf::MdWorld;
use mdpdf::fonts::FontOptions;
use mdpdf::settings::{self, Metadata, Settings};
use typst::diag::SourceDiagnostic;
use typst_kit::diagnostics::termcolor::{ColorChoice, StandardStream};
use typst_kit::diagnostics::{DiagnosticFormat, emit};

// Help text is user-facing, so it lives in `help`/`about` strings rather than doc comments.
#[derive(Debug, Parser)]
#[command(
    version,
    about = "把 Markdown 轉成 PDF",
    args_conflicts_with_subcommands = true,
    subcommand_negates_reqs = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    #[arg(required = true, help = "輸入的 Markdown 檔，`-` 代表 stdin")]
    input: Option<PathBuf>,

    #[arg(short, long, help = "輸出路徑（預設：輸入檔名.pdf；`-` 代表 stdout）")]
    output: Option<PathBuf>,

    #[arg(long, value_name = "FILE", help = "另存中間產生的 Typst 原始碼")]
    emit_typst: Option<PathBuf>,

    #[arg(long, help = "警告視為錯誤（例如找不到圖片或字型）")]
    strict: bool,

    #[command(flatten)]
    style: StyleArgs,

    #[command(flatten)]
    fonts: FontArgs,
}

#[derive(Debug, Subcommand)]
enum Command {
    #[command(about = "列出所有可用的字型名稱")]
    Fonts {
        #[command(flatten)]
        fonts: FontArgs,
    },
}

#[derive(Debug, Args)]
struct FontArgs {
    #[arg(long, value_name = "PATH", help = "額外字型檔或目錄（可重複指定）")]
    font_path: Vec<PathBuf>,

    #[arg(long, help = "不讀取系統字型")]
    no_system_fonts: bool,

    #[arg(
        long,
        value_name = "FILE",
        help = "設定檔路徑（預設：~/.config/mdpdf/config.toml）"
    )]
    config: Option<PathBuf>,

    #[arg(long, conflicts_with = "config", help = "不讀取設定檔")]
    no_config: bool,
}

#[derive(Debug, Args)]
struct StyleArgs {
    #[arg(long, value_name = "NAME", help = "內文字型（拉丁字母）")]
    font: Option<String>,

    #[arg(long, value_name = "NAME", help = "中文字型")]
    cjk_font: Option<String>,

    #[arg(long, value_name = "NAME", help = "程式碼字型")]
    mono_font: Option<String>,

    #[arg(
        long,
        value_name = "SIZE",
        help = "紙張大小，例如 a4、a5、us-letter（預設 a4）"
    )]
    paper: Option<String>,

    #[arg(
        long,
        value_name = "LEN",
        help = "頁邊距，例如 2cm、20mm、1in（預設 2.5cm）"
    )]
    margin: Option<String>,

    #[arg(long, help = "在標題後產生目錄")]
    toc: bool,

    #[arg(long, help = "標題加上編號（1、1.1、1.1.1）")]
    number_headings: bool,

    #[arg(long, value_name = "FILE", help = "語法高亮主題（.tmTheme）")]
    code_theme: Option<PathBuf>,

    #[arg(long, value_name = "FILE", help = "自訂 Typst 樣式模板，取代內建模板")]
    template: Option<PathBuf>,

    #[arg(
        long,
        help = "關閉 CJK 寬鬆強調，完全依照 CommonMark 規則判斷 `**` 能否成立"
    )]
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
    if let Some(Command::Fonts { fonts }) = &cli.command {
        return list_fonts(fonts);
    }
    let input = cli
        .input
        .clone()
        .expect("clap requires an input without a subcommand");
    let cwd = std::env::current_dir()?;

    let stdin = input == Path::new("-");
    let (markdown, dir, name) = if stdin {
        let mut text = String::new();
        io::stdin()
            .read_to_string(&mut text)
            .context("無法讀取 stdin")?;
        (text, cwd.clone(), "stdin".to_string())
    } else {
        let text =
            fs::read_to_string(&input).with_context(|| format!("無法讀取 {}", input.display()))?;
        let dir = input
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let name = input.file_name().unwrap().to_string_lossy().into_owned();
        (text, dir.to_path_buf(), name)
    };
    let dir = dir
        .canonicalize()
        .with_context(|| format!("無法解析目錄 {}", dir.display()))?;

    let output = match (&cli.output, stdin) {
        (Some(path), _) => path.clone(),
        (None, false) => input.with_extension("pdf"),
        (None, true) => bail!("從 stdin 讀取時請用 -o 指定輸出路徑"),
    };

    // Settings layers: command line > front matter > config file.
    let mut warnings = Vec::new();
    let (meta, front) = match mdparse::front_matter(&markdown) {
        Some(yaml) => settings::parse_front_matter(yaml, &dir).unwrap_or_else(|err| {
            warnings.push(format!("{err:#}，已忽略 front matter"));
            (Metadata::default(), Settings::default())
        }),
        None => (Metadata::default(), Settings::default()),
    };
    let merged = cli_settings(&cli.style, &cli.fonts, &cwd)
        .or(front)
        .or(config_settings(&cli.fonts)?);

    let fonts = mdpdf::fonts::load(&FontOptions {
        font_paths: merged.font_path.clone().unwrap_or_default(),
        system_fonts: merged.system_fonts.unwrap_or(true),
    })?;
    let resolved = settings::resolve(&merged, meta, fonts.book())?;
    warnings.extend(resolved.warnings);

    let options = md2typst::Options {
        base_dir: Some(dir.clone()),
        parse: resolved.parse,
        style: resolved.style,
        template: resolved.template,
        ..Default::default()
    };
    let main_path = dir.join(format!("{name}.typ"));
    let rendered = mdpdf::render(&markdown, &main_path, options, fonts)?;

    if let Some(path) = &cli.emit_typst {
        fs::write(path, &rendered.source)
            .with_context(|| format!("無法寫入 {}", path.display()))?;
    }
    for warning in &warnings {
        eprintln!("警告：{warning}");
    }
    for warning in &rendered.warnings {
        eprintln!("警告：{name}:{}：{}", warning.line, warning.message);
    }
    print_diagnostics(&rendered.world, &rendered.result.warnings)?;
    let pdf = match rendered.result.output {
        Ok(pdf) => pdf,
        Err(errors) => {
            print_diagnostics(&rendered.world, &errors)?;
            return Ok(ExitCode::FAILURE);
        }
    };
    let warning_count = warnings.len() + rendered.warnings.len();
    if cli.strict && warning_count > 0 {
        bail!("有 {warning_count} 個警告，--strict 模式下停止");
    }

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

/// The command-line layer. Flags that were not given stay unset so lower layers can fill them.
fn cli_settings(style: &StyleArgs, fonts: &FontArgs, cwd: &Path) -> Settings {
    Settings {
        font: style.font.clone(),
        cjk_font: style.cjk_font.clone(),
        mono_font: style.mono_font.clone(),
        font_path: (!fonts.font_path.is_empty()).then(|| fonts.font_path.clone()),
        system_fonts: fonts.no_system_fonts.then_some(false),
        paper: style.paper.clone(),
        margin: style.margin.clone(),
        toc: style.toc.then_some(true),
        number_headings: style.number_headings.then_some(true),
        code_theme: style.code_theme.clone(),
        template: style.template.clone(),
        cjk_emphasis: style.no_cjk_emphasis.then_some(false),
    }
    .resolve_paths(cwd)
}

/// The config file layer: `--config`, else the default path if it exists.
fn config_settings(fonts: &FontArgs) -> Result<Settings> {
    if fonts.no_config {
        return Ok(Settings::default());
    }
    match &fonts.config {
        Some(path) => settings::load_config(path),
        None => match settings::default_config_path().filter(|p| p.is_file()) {
            Some(path) => settings::load_config(&path),
            None => Ok(Settings::default()),
        },
    }
}

fn list_fonts(args: &FontArgs) -> Result<ExitCode> {
    let cwd = std::env::current_dir()?;
    let merged = cli_settings(&StyleArgs::none(), args, &cwd).or(config_settings(args)?);
    let fonts = mdpdf::fonts::load(&FontOptions {
        font_paths: merged.font_path.unwrap_or_default(),
        system_fonts: merged.system_fonts.unwrap_or(true),
    })?;
    let families: BTreeSet<&str> = fonts.book().families().map(|(family, _)| family).collect();
    let mut stdout = io::stdout().lock();
    for family in families {
        writeln!(stdout, "{family}")?;
    }
    Ok(ExitCode::SUCCESS)
}

impl StyleArgs {
    fn none() -> Self {
        Self {
            font: None,
            cjk_font: None,
            mono_font: None,
            paper: None,
            margin: None,
            toc: false,
            number_headings: false,
            code_theme: None,
            template: None,
            no_cjk_emphasis: false,
        }
    }
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
