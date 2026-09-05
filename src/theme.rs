use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::SystemTime,
};

use gpui::{App, Global, Pixels, px};
use serde::Deserialize;

#[cfg(target_os = "macos")]
pub const FONT_FAMILY: &str = "Menlo";
#[cfg(target_os = "windows")]
pub const FONT_FAMILY: &str = "Cascadia Mono";
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub const FONT_FAMILY: &str = "monospace";

#[cfg(target_os = "macos")]
pub const UI_FONT_FAMILY: &str = "Menlo";
#[cfg(not(target_os = "macos"))]
pub const UI_FONT_FAMILY: &str = ".SystemUIFont";

pub const FONT_SIZE: f32 = 13.0;
pub const LINE_HEIGHT: f32 = 1.35;
pub const SIDEBAR_WIDTH: f32 = 220.0;
pub const TERMINAL_PAD: Pixels = px(8.0);

pub const DEFAULT_THEME: &str = "tokyo-night";
pub const DEFAULT_THEMES_FILE: &str = "themes.toml";
pub const DEFAULT_THEMES_TOML: &str = include_str!("../themes.toml");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Colors {
    pub window: u32,
    pub sidebar: u32,
    pub sidebar_border: u32,
    pub tab_hover: u32,
    pub tab_active: u32,
    pub accent: u32,
    pub text: u32,
    pub text_dim: u32,
    pub cursor: u32,
    pub button: u32,
    pub tooltip: u32,
    pub term_fg: u32,
    pub term_bg: u32,
    pub ansi: [u32; 16],
}

impl Colors {
    pub fn rgb_parts(color: u32) -> (u8, u8, u8) {
        (((color >> 16) & 0xff) as u8, ((color >> 8) & 0xff) as u8, (color & 0xff) as u8)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeEntry {
    pub id: String,
    pub label: String,
    pub colors: Colors,
}

#[derive(Clone, Debug)]
pub struct ThemeCatalog {
    pub path: PathBuf,
    pub themes: Vec<ThemeEntry>,
    pub error: Option<String>,
    mtime: Option<SystemTime>,
}

impl Global for ThemeCatalog {}

impl Default for ThemeCatalog {
    fn default() -> Self {
        embedded_catalog()
    }
}

impl ThemeCatalog {
    pub fn lookup(&self, id: &str) -> Colors {
        let key = normalize_id(id);
        self.themes
            .iter()
            .find(|theme| theme.id == key)
            .or_else(|| self.themes.iter().find(|theme| theme.id == DEFAULT_THEME))
            .or_else(|| self.themes.first())
            .map(|theme| theme.colors)
            .unwrap_or_else(fallback_colors)
    }

    pub fn label_for(&self, id: &str) -> String {
        let key = normalize_id(id);
        self.themes
            .iter()
            .find(|theme| theme.id == key)
            .map(|theme| theme.label.clone())
            .filter(|label| !label.is_empty())
            .unwrap_or_else(|| display_label(&key))
    }
}

pub fn colors(cx: &App) -> Colors {
    catalog(cx).lookup(&crate::config::current(cx).theme)
}

pub fn choices(cx: &App) -> Vec<(String, String)> {
    catalog(cx)
        .themes
        .iter()
        .map(|theme| (theme.id.clone(), theme.label.clone()))
        .collect()
}

pub fn label_for(id: &str, cx: &App) -> String {
    catalog(cx).label_for(id)
}

pub fn catalog(cx: &App) -> ThemeCatalog {
    cx.try_global::<ThemeCatalog>().cloned().unwrap_or_else(embedded_catalog)
}

pub fn normalize_id(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return DEFAULT_THEME.to_string();
    }
    trimmed.to_ascii_lowercase().replace(['_', ' '], "-")
}

pub fn resolved_path(config_path: &Path, themes_file: &str) -> PathBuf {
    let file = themes_file.trim();
    let file = if file.is_empty() { DEFAULT_THEMES_FILE } else { file };
    let path = PathBuf::from(file);
    if path.is_absolute() {
        path
    } else {
        config_path.parent().map(|dir| dir.join(&path)).unwrap_or(path)
    }
}

pub fn write_default(path: &Path) -> std::io::Result<()> {
    if path.exists() {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, DEFAULT_THEMES_TOML)
}

pub fn reload(cx: &mut App) {
    let config = crate::config::current(cx);
    let path = resolved_path(&crate::config::path(cx), &config.themes_file);
    cx.set_global(load_catalog(&path));
}

pub fn reload_if_stale(cx: &mut App) -> bool {
    let config = crate::config::current(cx);
    let path = resolved_path(&crate::config::path(cx), &config.themes_file);
    let mtime = path.metadata().and_then(|meta| meta.modified()).ok();
    let current = cx.try_global::<ThemeCatalog>();
    if current.is_some_and(|catalog| catalog.path == path && catalog.mtime == mtime) {
        return false;
    }
    cx.set_global(load_catalog(&path));
    true
}

pub fn parse(text: &str) -> Result<Vec<ThemeEntry>, String> {
    if is_blank_or_comments(text) {
        return Err("themes file is empty".into());
    }
    let file: ThemesFile = toml::from_str(text).map_err(|error| error.to_string())?;
    let mut themes = Vec::new();
    for (id, spec) in file.themes {
        let id = normalize_id(&id);
        if id.is_empty() {
            continue;
        }
        themes.push(ThemeEntry {
            label: spec
                .label
                .as_deref()
                .map(str::trim)
                .filter(|label| !label.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| display_label(&id)),
            colors: spec.into_colors()?,
            id,
        });
    }
    if themes.is_empty() {
        return Err("themes file has no themes".into());
    }
    Ok(themes)
}

fn load_catalog(path: &Path) -> ThemeCatalog {
    if path.exists() {
        match std::fs::read_to_string(path) {
            Ok(text) => match parse(&text) {
                Ok(themes) => {
                    return ThemeCatalog {
                        path: path.to_path_buf(),
                        themes,
                        error: None,
                        mtime: path.metadata().and_then(|meta| meta.modified()).ok(),
                    };
                }
                Err(error) => {
                    let mut catalog = embedded_catalog();
                    catalog.path = path.to_path_buf();
                    catalog.error = Some(error);
                    catalog.mtime = path.metadata().and_then(|meta| meta.modified()).ok();
                    return catalog;
                }
            },
            Err(error) => {
                let mut catalog = embedded_catalog();
                catalog.path = path.to_path_buf();
                catalog.error = Some(error.to_string());
                return catalog;
            }
        }
    }
    let mut catalog = embedded_catalog();
    catalog.path = path.to_path_buf();
    catalog
}

fn embedded_catalog() -> ThemeCatalog {
    ThemeCatalog {
        path: PathBuf::from(DEFAULT_THEMES_FILE),
        themes: parse(DEFAULT_THEMES_TOML).unwrap_or_else(|_| vec![fallback_entry()]),
        error: None,
        mtime: None,
    }
}

fn fallback_entry() -> ThemeEntry {
    ThemeEntry {
        id: DEFAULT_THEME.into(),
        label: "Tokyo Night".into(),
        colors: fallback_colors(),
    }
}

fn fallback_colors() -> Colors {
    Colors {
        window: 0x0f1115,
        sidebar: 0x14161c,
        sidebar_border: 0x2a2d35,
        tab_hover: 0x1d212b,
        tab_active: 0x2a3140,
        accent: 0x7aa2f7,
        text: 0xc0caf5,
        text_dim: 0x7a8299,
        cursor: 0xc0caf5,
        button: 0x1f2430,
        tooltip: 0x232833,
        term_fg: 0xc0caf5,
        term_bg: 0x1a1b26,
        ansi: [
            0x15161e, 0xf7768e, 0x9ece6a, 0xe0af68, 0x7aa2f7, 0xbb9af7, 0x7dcfff, 0xa9b1d6, 0x414868, 0xf7768e, 0x9ece6a,
            0xe0af68, 0x7aa2f7, 0xbb9af7, 0x7dcfff, 0xc0caf5,
        ],
    }
}

fn display_label(id: &str) -> String {
    id.split('-')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_blank_or_comments(text: &str) -> bool {
    text.lines().all(|line| {
        let trimmed = line.trim();
        trimmed.is_empty() || trimmed.starts_with('#')
    })
}

#[derive(Debug, Deserialize)]
struct ThemesFile {
    #[serde(flatten)]
    themes: BTreeMap<String, ThemeSpec>,
}

#[derive(Debug, Deserialize)]
struct ThemeSpec {
    label: Option<String>,
    window: ColorValue,
    sidebar: ColorValue,
    sidebar_border: ColorValue,
    tab_hover: ColorValue,
    tab_active: ColorValue,
    accent: ColorValue,
    text: ColorValue,
    text_dim: ColorValue,
    cursor: ColorValue,
    button: ColorValue,
    tooltip: ColorValue,
    term_fg: ColorValue,
    term_bg: ColorValue,
    ansi: [ColorValue; 16],
}

impl ThemeSpec {
    fn into_colors(self) -> Result<Colors, String> {
        Ok(Colors {
            window: self.window.resolve("window")?,
            sidebar: self.sidebar.resolve("sidebar")?,
            sidebar_border: self.sidebar_border.resolve("sidebar_border")?,
            tab_hover: self.tab_hover.resolve("tab_hover")?,
            tab_active: self.tab_active.resolve("tab_active")?,
            accent: self.accent.resolve("accent")?,
            text: self.text.resolve("text")?,
            text_dim: self.text_dim.resolve("text_dim")?,
            cursor: self.cursor.resolve("cursor")?,
            button: self.button.resolve("button")?,
            tooltip: self.tooltip.resolve("tooltip")?,
            term_fg: self.term_fg.resolve("term_fg")?,
            term_bg: self.term_bg.resolve("term_bg")?,
            ansi: [
                self.ansi[0].resolve("ansi[0]")?,
                self.ansi[1].resolve("ansi[1]")?,
                self.ansi[2].resolve("ansi[2]")?,
                self.ansi[3].resolve("ansi[3]")?,
                self.ansi[4].resolve("ansi[4]")?,
                self.ansi[5].resolve("ansi[5]")?,
                self.ansi[6].resolve("ansi[6]")?,
                self.ansi[7].resolve("ansi[7]")?,
                self.ansi[8].resolve("ansi[8]")?,
                self.ansi[9].resolve("ansi[9]")?,
                self.ansi[10].resolve("ansi[10]")?,
                self.ansi[11].resolve("ansi[11]")?,
                self.ansi[12].resolve("ansi[12]")?,
                self.ansi[13].resolve("ansi[13]")?,
                self.ansi[14].resolve("ansi[14]")?,
                self.ansi[15].resolve("ansi[15]")?,
            ],
        })
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
enum ColorValue {
    Int(i64),
    Text(String),
}

impl ColorValue {
    fn resolve(&self, field: &str) -> Result<u32, String> {
        match self {
            Self::Int(value) if (0..=0x00ff_ffff).contains(value) => Ok(*value as u32),
            Self::Int(value) if (0..=0xffff_ffffu32 as i64).contains(value) => Ok(*value as u32),
            Self::Text(text) => parse_hex(text).ok_or_else(|| format!("invalid color for {field}: {text}")),
            Self::Int(value) => Err(format!("invalid color for {field}: {value}")),
        }
    }
}

fn parse_hex(text: &str) -> Option<u32> {
    let trimmed = text
        .trim()
        .trim_start_matches('#')
        .trim_start_matches("0x")
        .trim_start_matches("0X");
    if trimmed.len() != 6 {
        return None;
    }
    u32::from_str_radix(trimmed, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_file_loads_builtin_themes() {
        let themes = parse(DEFAULT_THEMES_TOML).unwrap();
        let ids: Vec<_> = themes.iter().map(|theme| theme.id.as_str()).collect();
        assert!(ids.contains(&"tokyo-night"));
        assert!(ids.contains(&"one-dark"));
        assert!(ids.contains(&"nord"));
        let night = themes.iter().find(|theme| theme.id == "tokyo-night").unwrap();
        let nord = themes.iter().find(|theme| theme.id == "nord").unwrap();
        assert_eq!(night.label, "Tokyo Night");
        assert_ne!(night.colors.window, nord.colors.window);
        assert_eq!(night.colors.ansi.len(), 16);
    }

    #[test]
    fn parses_hex_strings_and_falls_back_label() {
        let themes = parse(
            r##"
            [custom-dark]
            window = "#101010"
            sidebar = "0x202020"
            sidebar_border = 0x303030
            tab_hover = 0x404040
            tab_active = 0x505050
            accent = 0x6080ff
            text = 0xffffff
            text_dim = 0x888888
            cursor = 0xffffff
            button = 0x202020
            tooltip = 0x202020
            term_fg = 0xeeeeee
            term_bg = 0x101010
            ansi = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]
            "##,
        )
        .unwrap();
        assert_eq!(themes.len(), 1);
        assert_eq!(themes[0].id, "custom-dark");
        assert_eq!(themes[0].label, "Custom Dark");
        assert_eq!(themes[0].colors.window, 0x101010);
        assert_eq!(themes[0].colors.sidebar, 0x202020);
    }

    #[test]
    fn lookup_uses_default_when_missing() {
        let catalog = embedded_catalog();
        assert_eq!(catalog.lookup("tokyo-night").window, catalog.lookup("nope").window);
        assert_eq!(normalize_id("One Dark"), "one-dark");
        assert_eq!(normalize_id("  "), DEFAULT_THEME);
    }

    #[test]
    fn resolved_path_joins_config_dir() {
        let config = PathBuf::from("/tmp/ghostterm/config.toml");
        assert_eq!(resolved_path(&config, "themes.toml"), PathBuf::from("/tmp/ghostterm/themes.toml"));
        assert_eq!(resolved_path(&config, "/abs/custom.toml"), PathBuf::from("/abs/custom.toml"));
    }
}
