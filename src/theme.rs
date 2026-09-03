use gpui::{App, Pixels, px};

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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AppTheme {
    #[default]
    TokyoNight,
    CatppuccinMocha,
    GruvboxDark,
    OneDark,
    Nord,
    SolarizedLight,
}

impl AppTheme {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TokyoNight => "tokyo-night",
            Self::CatppuccinMocha => "catppuccin-mocha",
            Self::GruvboxDark => "gruvbox-dark",
            Self::OneDark => "one-dark",
            Self::Nord => "nord",
            Self::SolarizedLight => "solarized-light",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::TokyoNight => "Tokyo Night",
            Self::CatppuccinMocha => "Catppuccin Mocha",
            Self::GruvboxDark => "Gruvbox Dark",
            Self::OneDark => "One Dark",
            Self::Nord => "Nord",
            Self::SolarizedLight => "Solarized Light",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        let key = value.trim().to_ascii_lowercase().replace(['_', ' '], "-");
        match key.as_str() {
            "tokyo-night" | "tokyonight" | "tokyo" => Some(Self::TokyoNight),
            "catppuccin-mocha" | "catppuccin" | "mocha" => Some(Self::CatppuccinMocha),
            "gruvbox-dark" | "gruvbox" => Some(Self::GruvboxDark),
            "one-dark" | "onedark" => Some(Self::OneDark),
            "nord" => Some(Self::Nord),
            "solarized-light" | "solarized" => Some(Self::SolarizedLight),
            _ => None,
        }
    }

    pub fn all() -> [Self; 6] {
        [
            Self::TokyoNight,
            Self::CatppuccinMocha,
            Self::GruvboxDark,
            Self::OneDark,
            Self::Nord,
            Self::SolarizedLight,
        ]
    }
}

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

pub fn colors(cx: &App) -> Colors {
    named(crate::config::current(cx).theme)
}

pub fn named(id: AppTheme) -> Colors {
    match id {
        AppTheme::TokyoNight => tokyo_night(),
        AppTheme::CatppuccinMocha => catppuccin_mocha(),
        AppTheme::GruvboxDark => gruvbox_dark(),
        AppTheme::OneDark => one_dark(),
        AppTheme::Nord => nord(),
        AppTheme::SolarizedLight => solarized_light(),
    }
}

fn tokyo_night() -> Colors {
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

fn catppuccin_mocha() -> Colors {
    Colors {
        window: 0x11111b,
        sidebar: 0x181825,
        sidebar_border: 0x313244,
        tab_hover: 0x313244,
        tab_active: 0x45475a,
        accent: 0x89b4fa,
        text: 0xcdd6f4,
        text_dim: 0x6c7086,
        cursor: 0xf5e0dc,
        button: 0x313244,
        tooltip: 0x313244,
        term_fg: 0xcdd6f4,
        term_bg: 0x1e1e2e,
        ansi: [
            0x45475a, 0xf38ba8, 0xa6e3a1, 0xf9e2af, 0x89b4fa, 0xf5c2e7, 0x94e2d5, 0xbac2de, 0x585b70, 0xf38ba8, 0xa6e3a1,
            0xf9e2af, 0x89b4fa, 0xf5c2e7, 0x94e2d5, 0xa6adc8,
        ],
    }
}

fn gruvbox_dark() -> Colors {
    Colors {
        window: 0x1d2021,
        sidebar: 0x282828,
        sidebar_border: 0x3c3836,
        tab_hover: 0x3c3836,
        tab_active: 0x504945,
        accent: 0xfe8019,
        text: 0xebdbb2,
        text_dim: 0x928374,
        cursor: 0xebdbb2,
        button: 0x3c3836,
        tooltip: 0x32302f,
        term_fg: 0xebdbb2,
        term_bg: 0x282828,
        ansi: [
            0x282828, 0xcc241d, 0x98971a, 0xd79921, 0x458588, 0xb16286, 0x689d6a, 0xa89984, 0x928374, 0xfb4934, 0xb8bb26,
            0xfabd2f, 0x83a598, 0xd3869b, 0x8ec07c, 0xebdbb2,
        ],
    }
}

fn one_dark() -> Colors {
    Colors {
        window: 0x21252b,
        sidebar: 0x282c34,
        sidebar_border: 0x3e4451,
        tab_hover: 0x2c313c,
        tab_active: 0x3e4451,
        accent: 0x61afef,
        text: 0xabb2bf,
        text_dim: 0x5c6370,
        cursor: 0x528bff,
        button: 0x3e4451,
        tooltip: 0x2c313c,
        term_fg: 0xabb2bf,
        term_bg: 0x282c34,
        ansi: [
            0x282c34, 0xe06c75, 0x98c379, 0xe5c07b, 0x61afef, 0xc678dd, 0x56b6c2, 0xabb2bf, 0x5c6370, 0xe06c75, 0x98c379,
            0xe5c07b, 0x61afef, 0xc678dd, 0x56b6c2, 0xffffff,
        ],
    }
}

fn nord() -> Colors {
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

fn solarized_light() -> Colors {
    Colors {
        window: 0xfdf6e3,
        sidebar: 0xeee8d5,
        sidebar_border: 0x93a1a1,
        tab_hover: 0xe8e0c8,
        tab_active: 0xdcd3b6,
        accent: 0x268bd2,
        text: 0x657b83,
        text_dim: 0x93a1a1,
        cursor: 0x657b83,
        button: 0xeee8d5,
        tooltip: 0xeee8d5,
        term_fg: 0x657b83,
        term_bg: 0xfdf6e3,
        ansi: [
            0x073642, 0xdc322f, 0x859900, 0xb58900, 0x268bd2, 0xd33682, 0x2aa198, 0xeee8d5, 0x002b36, 0xcb4b16, 0x586e75,
            0x657b83, 0x839496, 0x6c71c4, 0x93a1a1, 0xfdf6e3,
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ids_and_aliases() {
        assert_eq!(AppTheme::parse("tokyo-night"), Some(AppTheme::TokyoNight));
        assert_eq!(AppTheme::parse("Catppuccin Mocha"), Some(AppTheme::CatppuccinMocha));
        assert_eq!(AppTheme::parse("gruvbox_dark"), Some(AppTheme::GruvboxDark));
        assert_eq!(AppTheme::parse("onedark"), Some(AppTheme::OneDark));
        assert_eq!(AppTheme::parse("nord"), Some(AppTheme::Nord));
        assert_eq!(AppTheme::parse("solarized"), Some(AppTheme::SolarizedLight));
        assert_eq!(AppTheme::parse("not-a-theme"), None);
    }

    #[test]
    fn named_themes_have_distinct_chrome() {
        let night = named(AppTheme::TokyoNight);
        let light = named(AppTheme::SolarizedLight);
        assert_ne!(night.window, light.window);
        assert_ne!(night.term_bg, light.term_bg);
        assert_eq!(night.ansi.len(), 16);
    }
}
