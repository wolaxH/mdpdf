//! Settings from the config file, the front matter and the command line.
//!
//! Precedence: command line > front matter > config file > defaults. Every layer is a
//! [`Settings`] with all fields optional, and [`Settings::or`] fills the gaps from a lower layer.
//! [`resolve`] then validates the merged settings and turns them into a [`md2typst::Style`].

use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use typst::layout::Paper;
use typst::text::FontBook;

/// The CJK font embedded in the binary, kept as a fallback whenever the fonts are overridden.
pub const DEFAULT_CJK_FONT: &str = "Noto Sans TC";
/// The template's default monospace font.
pub const DEFAULT_MONO_FONT: &str = "DejaVu Sans Mono";

/// One layer of settings. Keys use kebab-case in config files and front matter.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Settings {
    pub font: Option<String>,
    pub cjk_font: Option<String>,
    pub mono_font: Option<String>,
    pub font_path: Option<Vec<PathBuf>>,
    pub system_fonts: Option<bool>,
    pub paper: Option<String>,
    pub margin: Option<String>,
    pub toc: Option<bool>,
    pub number_headings: Option<bool>,
    pub code_theme: Option<PathBuf>,
    pub template: Option<PathBuf>,
    pub cjk_emphasis: Option<bool>,
}

/// Every key accepted in a config file, used to reject typos.
const KEYS: &[&str] = &[
    "font",
    "cjk-font",
    "mono-font",
    "font-path",
    "system-fonts",
    "paper",
    "margin",
    "toc",
    "number-headings",
    "code-theme",
    "template",
    "cjk-emphasis",
];

impl Settings {
    /// Fill every unset field from `lower`.
    pub fn or(self, lower: Settings) -> Settings {
        Settings {
            font: self.font.or(lower.font),
            cjk_font: self.cjk_font.or(lower.cjk_font),
            mono_font: self.mono_font.or(lower.mono_font),
            font_path: self.font_path.or(lower.font_path),
            system_fonts: self.system_fonts.or(lower.system_fonts),
            paper: self.paper.or(lower.paper),
            margin: self.margin.or(lower.margin),
            toc: self.toc.or(lower.toc),
            number_headings: self.number_headings.or(lower.number_headings),
            code_theme: self.code_theme.or(lower.code_theme),
            template: self.template.or(lower.template),
            cjk_emphasis: self.cjk_emphasis.or(lower.cjk_emphasis),
        }
    }

    /// Make relative paths absolute, relative to `base` (the directory of the file they came from).
    pub fn resolve_paths(mut self, base: &Path) -> Self {
        let abs = |p: PathBuf| if p.is_absolute() { p } else { base.join(p) };
        self.font_path = self
            .font_path
            .map(|paths| paths.into_iter().map(abs).collect());
        self.code_theme = self.code_theme.map(abs);
        self.template = self.template.map(abs);
        self
    }
}

/// `$XDG_CONFIG_HOME/mdpdf/config.toml`, or `~/.config/mdpdf/config.toml`.
pub fn default_config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("mdpdf/config.toml"))
}

/// Load a TOML config file. Unknown keys are rejected so typos do not go unnoticed.
pub fn load_config(path: &Path) -> Result<Settings> {
    let text =
        fs::read_to_string(path).with_context(|| format!("無法讀取設定檔 {}", path.display()))?;
    let table: toml::Table =
        toml::from_str(&text).with_context(|| format!("設定檔 {} 格式錯誤", path.display()))?;
    if let Some(key) = table.keys().find(|k| !KEYS.contains(&k.as_str())) {
        bail!("設定檔 {} 中有未知的設定 `{key}`", path.display());
    }
    let settings: Settings = table
        .try_into()
        .with_context(|| format!("設定檔 {} 格式錯誤", path.display()))?;
    let base = path.parent().unwrap_or(Path::new("."));
    Ok(settings.resolve_paths(base))
}

/// Document metadata from the front matter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Metadata {
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub date: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Authors {
    One(String),
    Many(Vec<String>),
}

#[derive(Deserialize)]
struct FrontMatter {
    title: Option<String>,
    author: Option<Authors>,
    date: Option<String>,
    // Unknown keys (tags, categories, ...) are common in front matter and ignored.
    #[serde(flatten)]
    settings: Settings,
}

/// Parse YAML front matter into metadata and settings. Relative paths resolve against `base`.
pub fn parse_front_matter(yaml: &str, base: &Path) -> Result<(Metadata, Settings)> {
    let front: FrontMatter = serde_saphyr::from_str(yaml).context("front matter 格式錯誤")?;
    let authors = match front.author {
        None => Vec::new(),
        Some(Authors::One(author)) => vec![author],
        Some(Authors::Many(authors)) => authors,
    };
    let meta = Metadata {
        title: front.title,
        authors,
        date: front.date,
    };
    Ok((meta, front.settings.resolve_paths(base)))
}

/// Validated settings ready for codegen.
#[derive(Debug)]
pub struct Resolved {
    pub style: md2typst::Style,
    pub template: Option<String>,
    pub parse: mdparse::Options,
    /// Non-fatal problems, such as a requested font that is not installed.
    pub warnings: Vec<String>,
}

/// Validate merged settings against the available fonts and build the codegen style.
///
/// Invalid values (unknown paper, malformed margin, missing theme or template file) are errors;
/// a missing font only produces a warning and falls back to the default font.
pub fn resolve(settings: &Settings, meta: Metadata, book: &FontBook) -> Result<Resolved> {
    let mut warnings = Vec::new();
    let mut available = |name: &str| {
        let found = book.contains_family(&name.to_lowercase());
        if !found {
            warnings.push(format!("找不到字型「{name}」，改用預設字型"));
        }
        found
    };

    let font = settings.font.as_deref().filter(|f| available(f));
    let cjk = settings.cjk_font.as_deref().filter(|f| available(f));
    let mono = settings.mono_font.as_deref().filter(|f| available(f));
    let cjk_fallback = cjk.unwrap_or(DEFAULT_CJK_FONT);

    // Keep a CJK font after any Latin font so CJK text never falls back to tofu.
    let body_fonts = match (font, cjk) {
        (None, None) => Vec::new(),
        (font, _) => font
            .into_iter()
            .chain([cjk_fallback])
            .map(String::from)
            .collect(),
    };
    let mono_fonts = match (mono, cjk) {
        (None, None) => Vec::new(),
        (mono, _) => [mono.unwrap_or(DEFAULT_MONO_FONT), cjk_fallback]
            .map(String::from)
            .to_vec(),
    };

    if let Some(paper) = &settings.paper
        && Paper::from_str(paper).is_err()
    {
        bail!("未知的紙張大小「{paper}」（例如 a4、a5、us-letter）");
    }
    if let Some(margin) = &settings.margin
        && !is_length(margin)
    {
        bail!("無效的邊距「{margin}」（例如 2cm、20mm、1in、72pt）");
    }
    let code_theme = match &settings.code_theme {
        Some(path) if !path.is_file() => bail!("找不到程式碼主題檔 {}", path.display()),
        Some(path) => Some(path.to_string_lossy().into_owned()),
        None => None,
    };
    let template = match &settings.template {
        Some(path) => Some(
            fs::read_to_string(path).with_context(|| format!("無法讀取模板 {}", path.display()))?,
        ),
        None => None,
    };

    let style = md2typst::Style {
        body_fonts,
        mono_fonts,
        paper: settings.paper.clone(),
        margin: settings.margin.clone(),
        toc: settings.toc.unwrap_or(false),
        number_headings: settings.number_headings.unwrap_or(false),
        code_theme,
        title: meta.title,
        authors: meta.authors,
        date: meta.date,
    };
    let parse = mdparse::Options {
        cjk_emphasis: settings.cjk_emphasis.unwrap_or(true),
        ..Default::default()
    };
    Ok(Resolved {
        style,
        template,
        parse,
        warnings,
    })
}

/// A single Typst length with an absolute unit, e.g. `2.5cm`. Emitted verbatim into Typst code,
/// so anything else is rejected.
fn is_length(s: &str) -> bool {
    let Some(unit_start) = s.find(|c: char| c.is_ascii_alphabetic()) else {
        return false;
    };
    let (number, unit) = s.split_at(unit_start);
    matches!(unit, "pt" | "mm" | "cm" | "in" | "em")
        && number
            .parse::<f64>()
            .is_ok_and(|n| n >= 0.0 && n.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layers_fill_gaps_in_order() {
        let cli = Settings {
            paper: Some("a5".into()),
            ..Default::default()
        };
        let front = Settings {
            paper: Some("a4".into()),
            toc: Some(true),
            ..Default::default()
        };
        let config = Settings {
            toc: Some(false),
            margin: Some("2cm".into()),
            ..Default::default()
        };
        let merged = cli.or(front).or(config);
        assert_eq!(merged.paper.as_deref(), Some("a5"));
        assert_eq!(merged.toc, Some(true));
        assert_eq!(merged.margin.as_deref(), Some("2cm"));
    }

    #[test]
    fn front_matter_metadata_and_settings() {
        let yaml = "title: 季度報告\nauthor: [甲, 乙]\ndate: 2026-09-28\ntoc: true\ncode-theme: themes/x.tmTheme\ntags: [ignored]\n";
        let (meta, settings) = parse_front_matter(yaml, Path::new("/doc")).unwrap();
        assert_eq!(meta.title.as_deref(), Some("季度報告"));
        assert_eq!(meta.authors, ["甲", "乙"]);
        assert_eq!(meta.date.as_deref(), Some("2026-09-28"));
        assert_eq!(settings.toc, Some(true));
        assert_eq!(
            settings.code_theme,
            Some(PathBuf::from("/doc/themes/x.tmTheme"))
        );

        let (meta, _) = parse_front_matter("author: 單一作者\n", Path::new("/")).unwrap();
        assert_eq!(meta.authors, ["單一作者"]);
        assert!(parse_front_matter("title: [unclosed\n", Path::new("/")).is_err());
    }

    #[test]
    fn config_rejects_unknown_keys() {
        let dir = std::env::temp_dir().join(format!("mdpdf-config-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        fs::write(&path, "paper = \"a5\"\nfont-path = [\"fonts\"]\n").unwrap();
        let settings = load_config(&path).unwrap();
        assert_eq!(settings.paper.as_deref(), Some("a5"));
        assert_eq!(settings.font_path, Some(vec![dir.join("fonts")]));

        fs::write(&path, "papr = \"a5\"\n").unwrap();
        let err = load_config(&path).unwrap_err().to_string();
        assert!(err.contains("papr"), "{err}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn validation() {
        let book = FontBook::from_infos(typst_kit::fonts::embedded().map(|(_, info)| info));
        let resolve = |settings: Settings| resolve(&settings, Metadata::default(), &book);

        let ok = resolve(Settings {
            font: Some("Libertinus Serif".into()),
            mono_font: Some("No Such Mono".into()),
            paper: Some("us-letter".into()),
            margin: Some("1.5cm".into()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(ok.style.body_fonts, ["Libertinus Serif", DEFAULT_CJK_FONT]);
        // The missing monospace font falls back to the defaults with a warning
        assert!(ok.style.mono_fonts.is_empty());
        assert_eq!(ok.warnings.len(), 1);
        assert!(ok.warnings[0].contains("No Such Mono"));

        assert!(
            resolve(Settings {
                paper: Some("a99".into()),
                ..Default::default()
            })
            .is_err()
        );
        for bad in ["2", "cm", "2 cm", "2px", "-1cm", "2cm) #panic(", "1e999cm"] {
            let result = resolve(Settings {
                margin: Some(bad.into()),
                ..Default::default()
            });
            assert!(result.is_err(), "margin {bad:?} should be rejected");
        }
        assert!(
            resolve(Settings {
                code_theme: Some("/no/such.tmTheme".into()),
                ..Default::default()
            })
            .is_err()
        );
    }
}
