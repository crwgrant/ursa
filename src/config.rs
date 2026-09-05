use std::{
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, SystemTime},
};

use gpui::{App, Bounds, DisplayId, Global, Pixels, SharedString, Timer, point, px, size};
use serde::Deserialize;

use crate::{notify, pty, theme};

pub const DEFAULT_SCROLLBACK: u32 = 2000;
pub const FONT_SIZE_MIN: f32 = 8.0;
pub const FONT_SIZE_MAX: f32 = 48.0;
pub const SCROLLBACK_MIN: u32 = 100;
pub const SCROLLBACK_MAX: u32 = 100_000;
pub const WINDOW_MIN_WIDTH: f32 = 640.0;
pub const WINDOW_MIN_HEIGHT: f32 = 400.0;
const WINDOW_STATE_FILE: &str = "window.toml";

static LAST_WINDOW_FRAME: Mutex<Option<WindowFrame>> = Mutex::new(None);

#[derive(Clone, Debug, PartialEq)]
pub struct WindowFrame {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub display: Option<String>,
}

impl WindowFrame {
    fn from_window(window: &gpui::Window, cx: &App) -> Self {
        let bounds = window.window_bounds().get_bounds();
        Self {
            x: f32::from(bounds.origin.x),
            y: f32::from(bounds.origin.y),
            width: f32::from(bounds.size.width),
            height: f32::from(bounds.size.height),
            display: window
                .display(cx)
                .and_then(|display| display.uuid().ok())
                .map(|id| id.to_string()),
        }
    }

    fn to_bounds(&self) -> Bounds<Pixels> {
        Bounds {
            origin: point(px(self.x), px(self.y)),
            size: size(px(self.width), px(self.height)),
        }
    }

    fn sanitized(self) -> Option<Self> {
        if ![self.x, self.y, self.width, self.height].into_iter().all(f32::is_finite) {
            return None;
        }
        let display = self
            .display
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        Some(Self {
            x: self.x,
            y: self.y,
            width: self.width.max(WINDOW_MIN_WIDTH),
            height: self.height.max(WINDOW_MIN_HEIGHT),
            display,
        })
    }

    fn render(&self) -> String {
        let display = self
            .display
            .as_deref()
            .map(|id| format!("display = {}\n", toml_string(id)))
            .unwrap_or_default();
        format!(
            "\
# Last Ghostterm window position and size. Updated when you move or resize the window.
# `display` is the monitor UUID so the window reopens on the same screen.
x = {x}
y = {y}
width = {width}
height = {height}
{display}",
            x = format_number(self.x),
            y = format_number(self.y),
            width = format_number(self.width),
            height = format_number(self.height),
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CursorShape {
    Block,
    #[default]
    Bar,
}

impl CursorShape {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Block => "block",
            Self::Bar => "bar",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Block => "Block",
            Self::Bar => "Bar",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "block" => Some(Self::Block),
            "bar" | "beam" | "i-beam" | "ibeam" => Some(Self::Bar),
            _ => None,
        }
    }

    pub fn all() -> [Self; 2] {
        [Self::Block, Self::Bar]
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub font_family: Option<String>,
    pub font_size: f32,
    pub scrollback_lines: u32,
    pub cursor_shape: CursorShape,
    pub theme: String,
    pub themes_file: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            font_family: None,
            font_size: theme::FONT_SIZE,
            scrollback_lines: DEFAULT_SCROLLBACK,
            cursor_shape: CursorShape::Bar,
            theme: theme::DEFAULT_THEME.to_string(),
            themes_file: theme::DEFAULT_THEMES_FILE.to_string(),
        }
    }
}

impl Config {
    pub fn resolved_font_family(&self) -> String {
        self.font_family
            .as_deref()
            .map(str::trim)
            .filter(|family| !family.is_empty())
            .unwrap_or(theme::FONT_FAMILY)
            .to_string()
    }

    pub fn sanitize(&mut self) {
        if self
            .font_family
            .as_deref()
            .map(str::trim)
            .is_none_or(|family| family.is_empty())
        {
            self.font_family = None;
        } else if let Some(family) = self.font_family.as_mut() {
            *family = family.trim().to_string();
        }
        self.font_size = self.font_size.clamp(FONT_SIZE_MIN, FONT_SIZE_MAX);
        self.scrollback_lines = self.scrollback_lines.clamp(SCROLLBACK_MIN, SCROLLBACK_MAX);
        self.theme = theme::normalize_id(&self.theme);
        let themes_file = self.themes_file.trim();
        self.themes_file = if themes_file.is_empty() {
            theme::DEFAULT_THEMES_FILE.to_string()
        } else {
            themes_file.to_string()
        };
    }

    pub fn render(&self) -> String {
        let family = toml_string(&self.resolved_font_family());
        let size = format_number(self.font_size);
        format!(
            "\
# Ghostterm configuration.
# This file is owned by Ghostterm (not libghostty). Edit it here or use Ghostterm → Settings.
# Unknown keys are ignored so older Ghostterm versions stay compatible.

[font]
# Terminal typeface. Leave empty or omit to use the platform default.
family = {family}
# Size in points ({FONT_SIZE_MIN:.0}–{FONT_SIZE_MAX:.0}).
size = {size}

[appearance]
# Filename stem of a .conf in the themes folder (nord, one-dark, tokyo-night, …).
theme = {theme}
# Folder of Ghostty theme files, relative to this file or an absolute path.
themes = {themes}

[terminal]
# Lines of scrollback kept above the viewport ({SCROLLBACK_MIN}–{SCROLLBACK_MAX}).
scrollback_lines = {scrollback}
# Cursor shape: block or bar.
cursor = {cursor}
",
            theme = toml_string(&self.theme),
            themes = toml_string(&self.themes_file),
            scrollback = self.scrollback_lines,
            cursor = toml_string(self.cursor_shape.as_str()),
        )
    }

    pub fn save(&self, path: &Path) -> std::io::Result<Option<SystemTime>> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, self.render())?;
        Ok(mtime_of(path))
    }
}

#[derive(Debug, Default, Deserialize)]
struct FileConfig {
    #[serde(default)]
    font: FontSection,
    #[serde(default)]
    terminal: TerminalSection,
    #[serde(default)]
    appearance: AppearanceSection,
    theme: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct FontSection {
    family: Option<String>,
    size: Option<f32>,
}

#[derive(Debug, Default, Deserialize)]
struct TerminalSection {
    scrollback_lines: Option<u32>,
    cursor: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct AppearanceSection {
    theme: Option<String>,
    themes: Option<String>,
}

pub struct AppSettings {
    pub config: Config,
    pub path: PathBuf,
    pub load_error: Option<String>,
    mtime: Mutex<Option<SystemTime>>,
}

impl Global for AppSettings {}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            config: Config::default(),
            path: config_path().unwrap_or_else(|| PathBuf::from("config.toml")),
            load_error: None,
            mtime: Mutex::new(None),
        }
    }
}

pub fn init(cx: &mut App) {
    let loaded = load();
    let error = loaded.error.clone();
    cx.set_global(AppSettings {
        config: loaded.config,
        path: loaded.path,
        load_error: loaded.error,
        mtime: Mutex::new(loaded.mtime),
    });
    if let Some(error) = error {
        notify::show(cx, format!("Config file error: {error}"));
    }
    let _ = theme::write_default(&path(cx), &current(cx).themes_file);
    theme::reload(cx);
    start_watcher(cx);
}

pub fn current(cx: &App) -> Config {
    cx.try_global::<AppSettings>()
        .map(|settings| settings.config.clone())
        .unwrap_or_default()
}

pub fn font_family(cx: &App) -> SharedString {
    SharedString::from(current(cx).resolved_font_family())
}

pub fn font_size(cx: &App) -> f32 {
    current(cx).font_size
}

pub fn scrollback_lines(cx: &App) -> u32 {
    current(cx).scrollback_lines
}

pub fn cursor_shape(cx: &App) -> CursorShape {
    current(cx).cursor_shape
}

pub fn path(cx: &App) -> PathBuf {
    cx.try_global::<AppSettings>()
        .map(|settings| settings.path.clone())
        .unwrap_or_else(|| config_path().unwrap_or_else(|| PathBuf::from("config.toml")))
}

pub fn display_path(cx: &App) -> String {
    shorten_path(&path(cx))
}

pub fn load_error(cx: &App) -> Option<String> {
    cx.try_global::<AppSettings>()
        .and_then(|settings| settings.load_error.clone())
}

pub fn update(cx: &mut App, edit: impl FnOnce(&mut Config)) {
    let mut config = current(cx);
    edit(&mut config);
    config.sanitize();
    persist(cx, config, true);
}

pub fn reset(cx: &mut App) {
    persist(cx, Config::default(), true);
}

pub fn reload(cx: &mut App) -> Result<(), String> {
    match load_from(&path(cx)) {
        Ok(loaded) => {
            apply_loaded(cx, loaded);
            Ok(())
        }
        Err(error) => {
            if cx.has_global::<AppSettings>() {
                cx.global_mut::<AppSettings>().load_error = Some(error.clone());
            }
            Err(error)
        }
    }
}

pub fn ensure_file(cx: &mut App) -> Option<PathBuf> {
    let path = path(cx);
    if !path.exists() {
        match current(cx).save(&path) {
            Ok(mtime) => {
                if cx.has_global::<AppSettings>() {
                    *cx.global_mut::<AppSettings>().mtime.lock().unwrap() = mtime;
                }
            }
            Err(error) => {
                notify::show(cx, format!("Couldn't create config file: {error}"));
                return None;
            }
        }
    }
    let _ = theme::write_default(&path, &current(cx).themes_file);
    theme::reload(cx);
    Some(path)
}

pub fn parse(text: &str) -> Result<Config, String> {
    if is_blank_or_comments(text) {
        return Ok(Config::default());
    }
    let file: FileConfig = toml::from_str(text).map_err(|error| error.to_string())?;
    let mut config = Config {
        font_family: file.font.family,
        font_size: file.font.size.unwrap_or(theme::FONT_SIZE),
        scrollback_lines: file.terminal.scrollback_lines.unwrap_or(DEFAULT_SCROLLBACK),
        cursor_shape: file
            .terminal
            .cursor
            .as_deref()
            .and_then(CursorShape::parse)
            .unwrap_or_default(),
        theme: file
            .appearance
            .theme
            .as_deref()
            .or(file.theme.as_deref())
            .map(theme::normalize_id)
            .unwrap_or_else(|| theme::DEFAULT_THEME.to_string()),
        themes_file: file
            .appearance
            .themes
            .as_deref()
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .unwrap_or(theme::DEFAULT_THEMES_FILE)
            .to_string(),
    };
    config.sanitize();
    Ok(config)
}

pub fn font_choices(current: &str, installed: &[String]) -> Vec<String> {
    let mut choices = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut push = |name: &str| {
        let trimmed = name.trim();
        if trimmed.is_empty() || trimmed.starts_with('.') {
            return;
        }
        let key = trimmed.to_ascii_lowercase();
        if seen.insert(key) {
            choices.push(trimmed.to_string());
        }
    };

    for preset in font_presets() {
        push(preset);
    }
    push(current);
    let mut rest = installed.to_vec();
    rest.sort_by(|a, b| a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase()));
    for name in rest {
        push(&name);
    }
    choices
}

pub fn font_size_presets() -> &'static [f32] {
    &[
        8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 18.0, 20.0, 22.0, 24.0, 28.0, 32.0, 36.0, 42.0, 48.0,
    ]
}

pub fn parse_scrollback(text: &str) -> Option<u32> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed
        .parse::<u32>()
        .ok()
        .map(|lines| lines.clamp(SCROLLBACK_MIN, SCROLLBACK_MAX))
}

pub fn font_size_choices(current: f32) -> Vec<f32> {
    let current = current.clamp(FONT_SIZE_MIN, FONT_SIZE_MAX);
    let mut choices = font_size_presets().to_vec();
    if !choices.iter().any(|&size| (size - current).abs() < 0.001) {
        choices.push(current);
        choices.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    }
    choices
}

pub fn font_presets() -> &'static [&'static str] {
    #[cfg(target_os = "macos")]
    {
        &["Menlo", "Monaco", "SF Mono", "Courier", "JetBrains Mono"]
    }
    #[cfg(target_os = "windows")]
    {
        &["Cascadia Mono", "Consolas", "Courier New", "JetBrains Mono"]
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        &["monospace", "DejaVu Sans Mono", "JetBrains Mono", "Noto Sans Mono"]
    }
}

pub fn config_path() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join("config.toml"))
}

pub fn save_window_state(window: &gpui::Window, cx: &App) {
    let Some(frame) = WindowFrame::from_window(window, cx).sanitized() else {
        return;
    };
    if let Ok(mut last) = LAST_WINDOW_FRAME.lock() {
        if last.as_ref() == Some(&frame) {
            return;
        }
        *last = Some(frame.clone());
    }
    let path = window_state_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, frame.render());
}

pub fn restored_window(cx: &App) -> Option<(Bounds<Pixels>, Option<DisplayId>)> {
    let frame = load_window_frame()?;
    if let Ok(mut last) = LAST_WINDOW_FRAME.lock() {
        *last = Some(frame.clone());
    }
    let display_id = frame.display.as_deref().and_then(|uuid| display_id_for_uuid(cx, uuid));
    let bounds = match display_id.and_then(|id| cx.find_display(id)) {
        Some(display) => clamp_to_display(frame.to_bounds(), display.bounds()),
        None => clamp_window_bounds(frame.to_bounds(), cx),
    };
    Some((bounds, display_id))
}

fn window_state_path() -> PathBuf {
    config_dir()
        .map(|dir| dir.join(WINDOW_STATE_FILE))
        .unwrap_or_else(|| PathBuf::from(WINDOW_STATE_FILE))
}

fn load_window_frame() -> Option<WindowFrame> {
    let text = std::fs::read_to_string(window_state_path()).ok()?;
    parse_window_frame(&text)
}

fn parse_window_frame(text: &str) -> Option<WindowFrame> {
    let file: WindowFile = toml::from_str(text).ok()?;
    WindowFrame {
        x: file.x?,
        y: file.y?,
        width: file.width?,
        height: file.height?,
        display: file.display,
    }
    .sanitized()
}

fn display_id_for_uuid(cx: &App, uuid: &str) -> Option<DisplayId> {
    cx.displays().into_iter().find_map(|display| {
        display
            .uuid()
            .ok()
            .filter(|id| id.to_string().eq_ignore_ascii_case(uuid))
            .map(|_| display.id())
    })
}

fn clamp_to_display(bounds: Bounds<Pixels>, display: Bounds<Pixels>) -> Bounds<Pixels> {
    let min = size(px(WINDOW_MIN_WIDTH), px(WINDOW_MIN_HEIGHT));
    let width = bounds.size.width.max(min.width).min(display.size.width.max(min.width));
    let height = bounds.size.height.max(min.height).min(display.size.height.max(min.height));
    let local_display = Bounds {
        origin: point(px(0.0), px(0.0)),
        size: display.size,
    };
    let bounds = Bounds {
        origin: bounds.origin,
        size: size(width, height),
    };
    if is_usable_on_display(&bounds, &local_display) {
        bounds
    } else {
        Bounds::centered_at(local_display.center(), bounds.size)
    }
}

fn clamp_window_bounds(bounds: Bounds<Pixels>, cx: &App) -> Bounds<Pixels> {
    let min = size(px(WINDOW_MIN_WIDTH), px(WINDOW_MIN_HEIGHT));
    let width = bounds.size.width.max(min.width);
    let height = bounds.size.height.max(min.height);
    let bounds = Bounds {
        origin: bounds.origin,
        size: size(width, height),
    };
    let displays: Vec<_> = cx.displays().into_iter().map(|display| display.bounds()).collect();
    if displays.is_empty() || displays.iter().any(|display| is_usable_on_display(&bounds, display)) {
        return bounds;
    }
    Bounds::centered(None, bounds.size, cx)
}

fn is_usable_on_display(bounds: &Bounds<Pixels>, display: &Bounds<Pixels>) -> bool {
    let hit = bounds.intersect(display);
    f32::from(hit.size.width) >= 80.0 && f32::from(hit.size.height) >= 40.0
}

#[derive(Debug, Default, Deserialize)]
struct WindowFile {
    x: Option<f32>,
    y: Option<f32>,
    width: Option<f32>,
    height: Option<f32>,
    display: Option<String>,
}

fn config_dir() -> Option<PathBuf> {
    let preferred =
        preferred_config_dir(pty::home_dir().as_deref(), std::env::var_os("XDG_CONFIG_HOME").as_deref().map(Path::new))?;
    if preferred.exists() {
        return Some(preferred);
    }
    if let Some(legacy) = legacy_config_dir() {
        if legacy.exists() {
            return Some(legacy);
        }
    }
    Some(preferred)
}

fn preferred_config_dir(home: Option<&Path>, xdg_config_home: Option<&Path>) -> Option<PathBuf> {
    xdg_config_home
        .map(Path::to_path_buf)
        .or_else(|| home.map(|home| home.join(".config")))
        .map(|root| root.join("ghostterm"))
}

fn legacy_config_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        pty::home_dir().map(|home| home.join("Library/Application Support/Ghostterm"))
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA").map(|app_data| PathBuf::from(app_data).join("Ghostterm"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

struct Loaded {
    config: Config,
    path: PathBuf,
    mtime: Option<SystemTime>,
    error: Option<String>,
}

fn load() -> Loaded {
    let path = config_path().unwrap_or_else(|| PathBuf::from("config.toml"));
    if !path.exists() {
        return Loaded {
            config: Config::default(),
            path,
            mtime: None,
            error: None,
        };
    }
    match load_from(&path) {
        Ok(loaded) => loaded,
        Err(error) => Loaded {
            config: Config::default(),
            path,
            mtime: None,
            error: Some(error),
        },
    }
}

fn load_from(path: &Path) -> Result<Loaded, String> {
    let text = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let config = parse(&text)?;
    Ok(Loaded {
        config,
        path: path.to_path_buf(),
        mtime: mtime_of(path),
        error: None,
    })
}

fn persist(cx: &mut App, config: Config, write: bool) {
    let path = path(cx);
    let mut mtime = None;
    let mut error = None;
    if write {
        match config.save(&path) {
            Ok(saved) => {
                mtime = saved;
                let _ = theme::write_default(&path, &config.themes_file);
            }
            Err(err) => {
                error = Some(err.to_string());
                notify::show(cx, format!("Couldn't save settings: {err}"));
            }
        }
    }
    if cx.has_global::<AppSettings>() {
        let settings = cx.global_mut::<AppSettings>();
        settings.config = config;
        if mtime.is_some() {
            *settings.mtime.lock().unwrap() = mtime;
        }
        settings.load_error = error;
    } else {
        cx.set_global(AppSettings {
            config,
            path,
            load_error: error,
            mtime: Mutex::new(mtime),
        });
    }
    theme::reload(cx);
}

fn apply_loaded(cx: &mut App, loaded: Loaded) {
    if cx.has_global::<AppSettings>() {
        let settings = cx.global_mut::<AppSettings>();
        settings.config = loaded.config;
        settings.path = loaded.path;
        settings.load_error = loaded.error;
        *settings.mtime.lock().unwrap() = loaded.mtime;
    } else {
        cx.set_global(AppSettings {
            config: loaded.config,
            path: loaded.path,
            load_error: loaded.error,
            mtime: Mutex::new(loaded.mtime),
        });
    }
    theme::reload(cx);
}

fn start_watcher(cx: &mut App) {
    cx.spawn(async move |cx| {
        loop {
            Timer::after(Duration::from_secs(2)).await;
            let error = cx.update(reload_if_changed).ok().flatten();
            if let Some(error) = error {
                let _ = cx.update(|cx| notify::show(cx, format!("Config file error: {error}")));
            }
        }
    })
    .detach();
}

fn reload_if_changed(cx: &mut App) -> Option<String> {
    let (path, previous, current_config, current_error) = {
        let settings = cx.try_global::<AppSettings>()?;
        (
            settings.path.clone(),
            *settings.mtime.lock().unwrap(),
            settings.config.clone(),
            settings.load_error.clone(),
        )
    };
    if !path.exists() {
        return None;
    }
    let mtime = mtime_of(&path);
    if mtime == previous {
        theme::reload_if_stale(cx);
        return None;
    }
    match load_from(&path) {
        Ok(loaded) => {
            if current_config == loaded.config {
                if let Some(settings) = cx.try_global::<AppSettings>() {
                    *settings.mtime.lock().unwrap() = loaded.mtime;
                }
                theme::reload_if_stale(cx);
                return None;
            }
            apply_loaded(cx, loaded);
            None
        }
        Err(error) => {
            let is_new = current_error.as_deref() != Some(error.as_str());
            if cx.has_global::<AppSettings>() {
                let settings = cx.global_mut::<AppSettings>();
                *settings.mtime.lock().unwrap() = mtime;
                settings.load_error = Some(error.clone());
            }
            is_new.then_some(error)
        }
    }
}

fn mtime_of(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|meta| meta.modified()).ok()
}

fn is_blank_or_comments(text: &str) -> bool {
    text.lines().all(|line| {
        let trimmed = line.trim();
        trimmed.is_empty() || trimmed.starts_with('#')
    })
}

fn toml_string(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn format_number(value: f32) -> String {
    if (value - value.round()).abs() < 0.001 {
        format!("{:.0}", value.round())
    } else {
        format!("{value}")
    }
}

pub fn shorten_path(path: &Path) -> String {
    if let Some(home) = pty::home_dir() {
        if let Ok(rest) = path.strip_prefix(home) {
            return format!("~/{}", rest.display());
        }
    }
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_blank_file_uses_defaults() {
        assert_eq!(parse("").unwrap(), Config::default());
        assert_eq!(parse("   \n# just a comment\n").unwrap(), Config::default());
    }

    #[test]
    fn parse_full_file() {
        let config = parse(
            r#"
            [font]
            family = "JetBrains Mono"
            size = 16
            [terminal]
            scrollback_lines = 8000
            cursor = "bar"
            [appearance]
            theme = "nord"
            "#,
        )
        .unwrap();
        assert_eq!(config.font_family.as_deref(), Some("JetBrains Mono"));
        assert_eq!(config.font_size, 16.0);
        assert_eq!(config.scrollback_lines, 8000);
        assert_eq!(config.cursor_shape, CursorShape::Bar);
        assert_eq!(config.theme, "nord");
        assert_eq!(config.themes_file, theme::DEFAULT_THEMES_FILE);
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let config = parse("[font]\nsize = 14\nunknown = true\n[other]\nfoo = 1\n").unwrap();
        assert_eq!(config.font_size, 14.0);
        assert_eq!(config.scrollback_lines, DEFAULT_SCROLLBACK);
    }

    #[test]
    fn invalid_toml_is_error() {
        assert!(parse("font = [").is_err());
    }

    #[test]
    fn clamps_out_of_range_values() {
        let config = parse("[font]\nsize = 1000\n[terminal]\nscrollback_lines = 1\n").unwrap();
        assert_eq!(config.font_size, FONT_SIZE_MAX);
        assert_eq!(config.scrollback_lines, SCROLLBACK_MIN);
    }

    #[test]
    fn empty_family_uses_platform_default() {
        let config = parse("[font]\nfamily = \"  \"\n").unwrap();
        assert_eq!(config.resolved_font_family(), theme::FONT_FAMILY);
        assert_eq!(config.font_family, None);
    }

    #[test]
    fn render_round_trip() {
        let original = Config {
            font_family: Some("SF Mono".into()),
            font_size: 15.0,
            scrollback_lines: 4000,
            cursor_shape: CursorShape::Bar,
            theme: "catppuccin-mocha".into(),
            themes_file: "palettes.toml".into(),
        };
        let again = parse(&original.render()).unwrap();
        assert_eq!(again.resolved_font_family(), "SF Mono");
        assert_eq!(again.font_size, 15.0);
        assert_eq!(again.scrollback_lines, 4000);
        assert_eq!(again.cursor_shape, CursorShape::Bar);
        assert_eq!(again.theme, "catppuccin-mocha");
        assert_eq!(again.themes_file, "palettes.toml");
    }

    #[test]
    fn font_choices_puts_presets_first_and_keeps_custom() {
        let choices = font_choices("Custom Font", &["Zapfino".into(), "Menlo".into(), ".SystemUIFont".into(), "Arial".into()]);
        assert_eq!(choices[0], font_presets()[0]);
        assert!(choices.contains(&"Custom Font".to_string()));
        assert!(choices.contains(&"Arial".to_string()));
        assert!(!choices.iter().any(|name| name.starts_with('.')));
        let menlo = choices.iter().filter(|name| name.eq_ignore_ascii_case("Menlo")).count();
        assert_eq!(menlo, 1);
    }

    #[test]
    fn font_size_choices_inserts_custom_size_in_order() {
        let choices = font_size_choices(17.5);
        assert!(choices.contains(&13.0));
        assert!(choices.contains(&17.5));
        let seventeen = choices.iter().position(|&size| (size - 17.5).abs() < 0.001).unwrap();
        let sixteen = choices.iter().position(|&size| (size - 16.0).abs() < 0.001).unwrap();
        let eighteen = choices.iter().position(|&size| (size - 18.0).abs() < 0.001).unwrap();
        assert!(sixteen < seventeen && seventeen < eighteen);
        assert_eq!(
            font_size_choices(13.0)
                .iter()
                .filter(|&&size| (size - 13.0).abs() < 0.001)
                .count(),
            1
        );
    }

    #[test]
    fn parse_scrollback_clamps_and_rejects_invalid() {
        assert_eq!(parse_scrollback("2000"), Some(2000));
        assert_eq!(parse_scrollback("  8000  "), Some(8000));
        assert_eq!(parse_scrollback("1"), Some(SCROLLBACK_MIN));
        assert_eq!(parse_scrollback("999999"), Some(SCROLLBACK_MAX));
        assert_eq!(parse_scrollback(""), None);
        assert_eq!(parse_scrollback("abc"), None);
        assert_eq!(parse_scrollback("20.5"), None);
    }

    #[test]
    fn parse_cursor_shape_aliases_and_fallback() {
        assert_eq!(parse("[terminal]\ncursor = \"bar\"\n").unwrap().cursor_shape, CursorShape::Bar);
        assert_eq!(parse("[terminal]\ncursor = \"I-Beam\"\n").unwrap().cursor_shape, CursorShape::Bar);
        assert_eq!(parse("[terminal]\ncursor = \"block\"\n").unwrap().cursor_shape, CursorShape::Block);
        assert_eq!(parse("[terminal]\ncursor = \"squiggle\"\n").unwrap().cursor_shape, CursorShape::Bar);
    }

    #[test]
    fn parse_theme_from_appearance_or_top_level() {
        assert_eq!(parse("[appearance]\ntheme = \"gruvbox-dark\"\n").unwrap().theme, "gruvbox-dark");
        assert_eq!(parse("theme = \"solarized-light\"\n").unwrap().theme, "solarized-light");
        assert_eq!(parse("theme = \"nord\"\n[appearance]\ntheme = \"one-dark\"\n").unwrap().theme, "one-dark");
        assert_eq!(parse("[appearance]\ntheme = \"nope\"\n").unwrap().theme, "nope");
        assert_eq!(parse("[appearance]\nthemes = \"custom.toml\"\n").unwrap().themes_file, "custom.toml");
    }

    #[test]
    fn default_config_dir_is_xdg() {
        let home = PathBuf::from("/Users/dev");
        assert_eq!(
            preferred_config_dir(Some(&home), None).as_deref(),
            Some(Path::new("/Users/dev/.config/ghostterm"))
        );
        assert_eq!(
            preferred_config_dir(Some(&home), Some(Path::new("/custom/xdg"))).as_deref(),
            Some(Path::new("/custom/xdg/ghostterm"))
        );
        assert_eq!(preferred_config_dir(None, None), None);
    }

    #[test]
    fn window_frame_round_trip() {
        let parsed = parse_window_frame(
            "x = 120\ny = 80\nwidth = 1100\nheight = 720\ndisplay = \"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee\"\n",
        )
        .unwrap();
        assert_eq!(parsed.x, 120.0);
        assert_eq!(parsed.y, 80.0);
        assert_eq!(parsed.width, 1100.0);
        assert_eq!(parsed.height, 720.0);
        assert_eq!(parsed.display.as_deref(), Some("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"));
        let again = parse_window_frame(&parsed.render()).unwrap();
        assert_eq!(again, parsed);
    }

    #[test]
    fn window_frame_clamps_small_size_and_rejects_invalid() {
        let small = parse_window_frame("x = 10\ny = 10\nwidth = 200\nheight = 100\n").unwrap();
        assert_eq!(small.width, WINDOW_MIN_WIDTH);
        assert_eq!(small.height, WINDOW_MIN_HEIGHT);
        assert!(parse_window_frame("x = 1\ny = 2\n").is_none());
        assert!(parse_window_frame("x = nan\ny = 0\nwidth = 800\nheight = 600\n").is_none());
    }
}
