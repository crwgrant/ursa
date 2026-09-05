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

pub const DEFAULT_THEME: &str = "nord";
pub const DEFAULT_THEMES_FILE: &str = "themes.toml";
pub const DEFAULT_THEMES_DIR: &str = "themes";
pub const DEFAULT_THEMES_TOML: &str = include_str!("../themes.toml");
pub const DEFAULT_FRAPPE_CONF: &str = include_str!("../themes/catppuccin-frappe.conf");

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
    pub extra_dir: PathBuf,
    pub themes: Vec<ThemeEntry>,
    pub error: Option<String>,
    mtime: Option<SystemTime>,
    source_count: usize,
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

pub fn extra_themes_dir(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .map(|dir| dir.join(DEFAULT_THEMES_DIR))
        .unwrap_or_else(|| PathBuf::from(DEFAULT_THEMES_DIR))
}

pub fn write_default(config_path: &Path, themes_file: &str) -> std::io::Result<()> {
    let catalog = resolved_path(config_path, themes_file);
    if !catalog.exists() && catalog.extension().is_some() {
        if let Some(dir) = catalog.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&catalog, DEFAULT_THEMES_TOML)?;
    }
    let extra = extra_themes_dir(config_path);
    std::fs::create_dir_all(&extra)?;
    let sample = extra.join("catppuccin-frappe.conf");
    if !sample.exists() {
        std::fs::write(sample, DEFAULT_FRAPPE_CONF)?;
    }
    Ok(())
}

pub fn reload(cx: &mut App) {
    let config_path = crate::config::path(cx);
    let path = resolved_path(&config_path, &crate::config::current(cx).themes_file);
    cx.set_global(load_catalog(&path, &config_path));
}

pub fn reload_if_stale(cx: &mut App) -> bool {
    let config_path = crate::config::path(cx);
    let path = resolved_path(&config_path, &crate::config::current(cx).themes_file);
    let extra = extra_themes_dir(&config_path);
    let (mtime, source_count) = sources_stamp(&path, &extra);
    let current = cx.try_global::<ThemeCatalog>();
    if current.is_some_and(|catalog| {
        catalog.path == path && catalog.extra_dir == extra && catalog.mtime == mtime && catalog.source_count == source_count
    }) {
        return false;
    }
    cx.set_global(load_catalog(&path, &config_path));
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

pub fn parse_ghostty(text: &str, id: &str, label: &str) -> Result<ThemeEntry, String> {
    if is_blank_or_comments(text) {
        return Err("theme file is empty".into());
    }
    let mut background = None;
    let mut foreground = None;
    let mut cursor = None;
    let mut ansi = [None; 16];
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = split_key_value(line) else {
            continue;
        };
        match key {
            "background" => background = Some(parse_ghostty_color(value, "background")?),
            "foreground" => foreground = Some(parse_ghostty_color(value, "foreground")?),
            "cursor-color" => cursor = Some(parse_ghostty_color(value, "cursor-color")?),
            "palette" => {
                let Some((index, color)) = split_key_value(value) else {
                    return Err(format!("invalid palette entry: {value}"));
                };
                let index = parse_palette_index(index)?;
                if index < 16 {
                    ansi[index] = Some(parse_ghostty_color(color, &format!("palette[{index}]"))?);
                }
            }
            _ => {}
        }
    }
    let background = background.ok_or_else(|| "missing background".to_string())?;
    let foreground = foreground.ok_or_else(|| "missing foreground".to_string())?;
    let mut palette = fallback_colors().ansi;
    for (index, color) in ansi.into_iter().enumerate() {
        if let Some(color) = color {
            palette[index] = color;
        }
    }
    let id = normalize_id(id);
    let label = label.trim();
    let label = if label.is_empty() {
        display_label(&id)
    } else {
        label.to_string()
    };
    Ok(ThemeEntry {
        colors: derive_chrome(background, foreground, cursor.unwrap_or(foreground), palette),
        id,
        label,
    })
}

fn load_catalog(path: &Path, config_path: &Path) -> ThemeCatalog {
    let extra = extra_themes_dir(config_path);
    let (mtime, source_count) = sources_stamp(path, &extra);
    let mut themes = Vec::new();
    let mut errors = Vec::new();

    if path.is_dir() {
        load_theme_dir(path, &mut themes, &mut errors);
        if themes.is_empty() {
            merge_entries(&mut themes, embedded_themes());
        }
    } else if path.exists() {
        match load_theme_file(path) {
            Ok(entries) => merge_entries(&mut themes, entries),
            Err(error) => {
                errors.push(error);
                merge_entries(&mut themes, embedded_themes());
            }
        }
    } else {
        merge_entries(&mut themes, embedded_themes());
    }

    if extra.exists() && !same_path(path, &extra) {
        load_theme_dir(&extra, &mut themes, &mut errors);
    }

    if themes.is_empty() {
        themes = vec![fallback_entry()];
    }

    ThemeCatalog {
        path: path.to_path_buf(),
        extra_dir: extra,
        themes,
        error: join_errors(errors),
        mtime,
        source_count,
    }
}

fn embedded_catalog() -> ThemeCatalog {
    ThemeCatalog {
        path: PathBuf::from(DEFAULT_THEMES_FILE),
        extra_dir: PathBuf::from(DEFAULT_THEMES_DIR),
        themes: embedded_themes(),
        error: None,
        mtime: None,
        source_count: 0,
    }
}

fn embedded_themes() -> Vec<ThemeEntry> {
    parse(DEFAULT_THEMES_TOML).unwrap_or_else(|_| vec![fallback_entry()])
}

fn load_theme_dir(dir: &Path, themes: &mut Vec<ThemeEntry>, errors: &mut Vec<String>) {
    let mut files = match std::fs::read_dir(dir) {
        Ok(entries) => entries.flatten().map(|entry| entry.path()).collect::<Vec<_>>(),
        Err(error) => {
            errors.push(format!("{}: {error}", dir.display()));
            return;
        }
    };
    files.sort();
    for path in files {
        if !is_theme_source(&path) {
            continue;
        }
        match load_theme_file(&path) {
            Ok(entries) => merge_entries(themes, entries),
            Err(error) => errors.push(error),
        }
    }
}

fn load_theme_file(path: &Path) -> Result<Vec<ThemeEntry>, String> {
    let text = std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    if is_ghostty_source(path, &text) {
        let id = id_from_path(path);
        Ok(vec![
            parse_ghostty(&text, &id, &label_from_path(path)).map_err(|error| format!("{}: {error}", path.display()))?,
        ])
    } else {
        parse(&text).map_err(|error| format!("{}: {error}", path.display()))
    }
}

fn is_theme_source(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("");
    if name.starts_with('.') {
        return false;
    }
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("toml" | "conf") => true,
        Some(_) => false,
        None => true,
    }
}

fn is_ghostty_source(path: &Path, text: &str) -> bool {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("conf") => true,
        Some("toml") => false,
        _ => looks_like_ghostty(text),
    }
}

fn looks_like_ghostty(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return false;
        }
        matches!(
            split_key_value(line).map(|(key, _)| key),
            Some("palette" | "background" | "foreground" | "cursor-color")
        )
    })
}

fn merge_entries(themes: &mut Vec<ThemeEntry>, incoming: Vec<ThemeEntry>) {
    for theme in incoming {
        if let Some(existing) = themes.iter_mut().find(|entry| entry.id == theme.id) {
            *existing = theme;
        } else {
            themes.push(theme);
        }
    }
}

fn sources_stamp(primary: &Path, extra: &Path) -> (Option<SystemTime>, usize) {
    let mut mtime = None;
    let mut count = 0;
    bump_stamp(&mut mtime, &mut count, primary);
    if extra.exists() && !same_path(primary, extra) {
        bump_stamp(&mut mtime, &mut count, extra);
        if let Ok(entries) = std::fs::read_dir(extra) {
            for path in entries.flatten().map(|entry| entry.path()) {
                if is_theme_source(&path) {
                    bump_stamp(&mut mtime, &mut count, &path);
                }
            }
        }
    } else if primary.is_dir() {
        if let Ok(entries) = std::fs::read_dir(primary) {
            for path in entries.flatten().map(|entry| entry.path()) {
                if is_theme_source(&path) {
                    bump_stamp(&mut mtime, &mut count, &path);
                }
            }
        }
    }
    (mtime, count)
}

fn bump_stamp(mtime: &mut Option<SystemTime>, count: &mut usize, path: &Path) {
    if !path.exists() {
        return;
    }
    *count += 1;
    if let Ok(modified) = path.metadata().and_then(|meta| meta.modified()) {
        *mtime = Some(mtime.map_or(modified, |current| current.max(modified)));
    }
}

fn same_path(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn join_errors(errors: Vec<String>) -> Option<String> {
    if errors.is_empty() { None } else { Some(errors.join("; ")) }
}

fn fallback_entry() -> ThemeEntry {
    ThemeEntry {
        id: DEFAULT_THEME.into(),
        label: "Nord".into(),
        colors: fallback_colors(),
    }
}

fn fallback_colors() -> Colors {
    Colors {
        window: 0x2e3440,
        sidebar: 0x3b4252,
        sidebar_border: 0x4c566a,
        tab_hover: 0x434c5e,
        tab_active: 0x4c566a,
        accent: 0x88c0d0,
        text: 0xeceff4,
        text_dim: 0x81a1c1,
        cursor: 0xd8dee9,
        button: 0x434c5e,
        tooltip: 0x3b4252,
        term_fg: 0xd8dee9,
        term_bg: 0x2e3440,
        ansi: [
            0x3b4252, 0xbf616a, 0xa3be8c, 0xebcb8b, 0x81a1c1, 0xb48ead, 0x88c0d0, 0xe5e9f0, 0x4c566a, 0xbf616a, 0xa3be8c,
            0xebcb8b, 0x81a1c1, 0xb48ead, 0x8fbcbb, 0xeceff4,
        ],
    }
}

fn derive_chrome(background: u32, foreground: u32, cursor: u32, ansi: [u32; 16]) -> Colors {
    let dark = luminance(background) < 0.5;
    let toward_fg = if dark { 0xffffff } else { 0x000000 };
    let toward_bg = if dark { 0x000000 } else { 0xffffff };
    let text_dim = if color_distance(ansi[8], background) > 12.0 {
        ansi[8]
    } else {
        mix(foreground, background, 0.45)
    };
    Colors {
        window: mix(background, toward_bg, 0.14),
        sidebar: mix(background, toward_fg, 0.06),
        sidebar_border: mix(background, toward_fg, 0.18),
        tab_hover: mix(background, toward_fg, 0.08),
        tab_active: mix(background, toward_fg, 0.16),
        accent: ansi[4],
        text: foreground,
        text_dim,
        cursor,
        button: mix(background, toward_fg, 0.08),
        tooltip: mix(background, toward_fg, 0.10),
        term_fg: foreground,
        term_bg: background,
        ansi,
    }
}

fn luminance(color: u32) -> f32 {
    let (red, green, blue) = Colors::rgb_parts(color);
    (0.2126 * red as f32 + 0.7152 * green as f32 + 0.0722 * blue as f32) / 255.0
}

fn mix(from: u32, to: u32, amount: f32) -> u32 {
    let amount = amount.clamp(0.0, 1.0);
    let (fr, fg, fb) = Colors::rgb_parts(from);
    let (tr, tg, tb) = Colors::rgb_parts(to);
    let red = (fr as f32 + (tr as f32 - fr as f32) * amount).round() as u32;
    let green = (fg as f32 + (tg as f32 - fg as f32) * amount).round() as u32;
    let blue = (fb as f32 + (tb as f32 - fb as f32) * amount).round() as u32;
    (red << 16) | (green << 8) | blue
}

fn color_distance(left: u32, right: u32) -> f32 {
    let (lr, lg, lb) = Colors::rgb_parts(left);
    let (rr, rg, rb) = Colors::rgb_parts(right);
    let dr = lr as f32 - rr as f32;
    let dg = lg as f32 - rg as f32;
    let db = lb as f32 - rb as f32;
    (dr * dr + dg * dg + db * db).sqrt()
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

fn id_from_path(path: &Path) -> String {
    normalize_id(
        &path
            .file_stem()
            .or_else(|| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
    )
}

fn label_from_path(path: &Path) -> String {
    let stem = path
        .file_stem()
        .or_else(|| path.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let trimmed = stem.trim();
    if trimmed.chars().any(char::is_whitespace) {
        trimmed.to_string()
    } else {
        display_label(&normalize_id(trimmed))
    }
}

fn is_blank_or_comments(text: &str) -> bool {
    text.lines().all(|line| {
        let trimmed = line.trim();
        trimmed.is_empty() || trimmed.starts_with('#')
    })
}

fn split_key_value(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once('=')?;
    let key = key.trim();
    let value = value.trim();
    if key.is_empty() || value.is_empty() {
        None
    } else {
        Some((key, value))
    }
}

fn parse_ghostty_color(value: &str, field: &str) -> Result<u32, String> {
    let token = unquote(value)
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_matches(|ch| ch == '"' || ch == '\'');
    parse_hex(token).ok_or_else(|| format!("invalid color for {field}: {value}"))
}

fn unquote(value: &str) -> &str {
    let value = value.trim();
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        if (bytes[0] == b'"' && bytes[value.len() - 1] == b'"') || (bytes[0] == b'\'' && bytes[value.len() - 1] == b'\'') {
            return &value[1..value.len() - 1];
        }
    }
    value
}

fn parse_palette_index(value: &str) -> Result<usize, String> {
    let value = value.trim();
    let parsed = if let Some(hex) = value.strip_prefix("0x").or_else(|| value.strip_prefix("0X")) {
        usize::from_str_radix(hex, 16)
    } else {
        value.parse()
    };
    parsed.map_err(|_| format!("invalid palette index: {value}"))
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
        assert_eq!(catalog.lookup("nord").window, catalog.lookup("nope").window);
        assert_eq!(normalize_id("One Dark"), "one-dark");
        assert_eq!(normalize_id("Catppuccin Frappe"), "catppuccin-frappe");
        assert_eq!(normalize_id("  "), DEFAULT_THEME);
    }

    #[test]
    fn resolved_path_joins_config_dir() {
        let config = PathBuf::from("/tmp/ghostterm/config.toml");
        assert_eq!(resolved_path(&config, "themes.toml"), PathBuf::from("/tmp/ghostterm/themes.toml"));
        assert_eq!(resolved_path(&config, "/abs/custom.toml"), PathBuf::from("/abs/custom.toml"));
        assert_eq!(extra_themes_dir(&config), PathBuf::from("/tmp/ghostterm/themes"));
    }

    #[test]
    fn parses_ghostty_frappe_and_derives_chrome() {
        let theme = parse_ghostty(DEFAULT_FRAPPE_CONF, "Catppuccin Frappe", "Catppuccin Frappé").unwrap();
        assert_eq!(theme.id, "catppuccin-frappe");
        assert_eq!(theme.label, "Catppuccin Frappé");
        assert_eq!(theme.colors.term_bg, 0x303446);
        assert_eq!(theme.colors.term_fg, 0xc6d0f5);
        assert_eq!(theme.colors.cursor, 0xf2d5cf);
        assert_eq!(theme.colors.ansi[0], 0x51576d);
        assert_eq!(theme.colors.ansi[4], 0x8caaee);
        assert_eq!(theme.colors.ansi[15], 0xb5bfe2);
        assert_eq!(theme.colors.accent, 0x8caaee);
        assert_eq!(theme.colors.text, 0xc6d0f5);
        assert_ne!(theme.colors.window, theme.colors.term_bg);
        assert_ne!(theme.colors.sidebar, theme.colors.term_bg);
    }

    #[test]
    fn ghostty_parser_ignores_unknown_keys_and_high_palette() {
        let theme = parse_ghostty(
            r##"
            font-family = "Nope"
            background = "#112233"
            foreground = 445566
            cursor-color = "#778899"
            palette = 0=#010101
            palette = 4=#00aaff
            palette = 8=#334455
            palette = 16=#ffffff
            selection-background = #abcdef
            "##,
            "custom",
            "",
        )
        .unwrap();
        assert_eq!(theme.id, "custom");
        assert_eq!(theme.label, "Custom");
        assert_eq!(theme.colors.term_bg, 0x112233);
        assert_eq!(theme.colors.term_fg, 0x445566);
        assert_eq!(theme.colors.cursor, 0x778899);
        assert_eq!(theme.colors.ansi[4], 0x00aaff);
        assert_eq!(theme.colors.accent, 0x00aaff);
    }

    #[test]
    fn loads_ghostty_conf_from_themes_dir() {
        let root = std::env::temp_dir().join(format!("ghostterm-theme-loader-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("themes")).unwrap();
        let config = root.join("config.toml");
        let catalog_path = root.join("themes.toml");
        std::fs::write(&catalog_path, DEFAULT_THEMES_TOML).unwrap();
        std::fs::write(root.join("themes/catppuccin-frappe.conf"), DEFAULT_FRAPPE_CONF).unwrap();
        let catalog = load_catalog(&catalog_path, &config);
        let ids: Vec<_> = catalog.themes.iter().map(|theme| theme.id.as_str()).collect();
        assert!(ids.contains(&"nord"));
        assert!(ids.contains(&"catppuccin-frappe"));
        let frappe = catalog.themes.iter().find(|theme| theme.id == "catppuccin-frappe").unwrap();
        assert_eq!(frappe.colors.term_bg, 0x303446);
        let _ = std::fs::remove_dir_all(&root);
    }
}
