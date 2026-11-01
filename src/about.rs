use std::sync::{Arc, OnceLock};

use gpui::{
    App, Bounds, Context, DisplayId, FocusHandle, Global, Image, ImageFormat, IntoElement, KeyDownEvent, ObjectFit, Render,
    TitlebarOptions, Window, WindowBounds, WindowOptions, img, prelude::*, px, size,
};
use gpui::component::{h_flex, label::Label, link::Link, v_flex};

use crate::notify;

const APP_NAME: &str = "Ursa";
const VERSION: &str = env!("CARGO_PKG_VERSION");
const TAGLINE: &str = env!("CARGO_PKG_DESCRIPTION");
const GITHUB_URL: &str = env!("CARGO_PKG_REPOSITORY");
const ICON_PNG: &[u8] = include_bytes!("../packaging/AppIcon-small.png");

#[derive(Default)]
struct AboutUi {
    window: Option<gpui::AnyWindowHandle>,
}

impl Global for AboutUi {}

pub struct AboutPage {
    focus: FocusHandle,
}

impl AboutPage {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        Self { focus }
    }

    fn handle_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            window.remove_window();
            cx.stop_propagation();
        }
    }

    fn close_window(&mut self, window: &mut Window, _cx: &mut Context<Self>) {
        window.remove_window();
    }
}

pub fn open(cx: &mut App) {
    if let Some(handle) = cx.try_global::<AboutUi>().and_then(|ui| ui.window) {
        if crate::activate_window(handle, cx) {
            return;
        }
    }

    let display_id = crate::host_display_id(cx);
    match gpui::open_window(window_options(cx, display_id), cx, |window, cx| cx.new(|cx| AboutPage::new(window, cx))) {
        Ok((handle, _)) => {
            crate::on_aux_window_close(handle, cx, |cx| {
                if cx.has_global::<AboutUi>() {
                    cx.global_mut::<AboutUi>().window = None;
                }
            });
            cx.default_global::<AboutUi>().window = Some(handle);
        }
        Err(error) => notify::show(cx, format!("Couldn't open About: {error}")),
    }
}

fn window_options(cx: &App, display_id: Option<DisplayId>) -> WindowOptions {
    let bounds = Bounds::centered(display_id, size(px(400.0), px(460.0)), cx);
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some("About Ursa".into()),
            appears_transparent: false,
            traffic_light_position: None,
        }),
        focus: true,
        show: true,
        kind: gpui::WindowKind::Normal,
        is_movable: true,
        display_id,
        window_min_size: Some(size(px(380.0), px(420.0))),
        window_background: gpui::WindowBackgroundAppearance::Opaque,
        app_id: Some(crate::APP_ID.into()),
        is_resizable: false,
        is_minimizable: true,
        window_decorations: None,
        tabbing_identifier: Some("Ursa About".into()),
        ..Default::default()
    }
}

fn app_icon() -> Arc<Image> {
    static ICON: OnceLock<Arc<Image>> = OnceLock::new();
    ICON.get_or_init(|| Arc::new(Image::from_bytes(ImageFormat::Png, ICON_PNG.to_vec())))
        .clone()
}

fn github_label() -> &'static str {
    GITHUB_URL
        .strip_prefix("https://")
        .or_else(|| GITHUB_URL.strip_prefix("http://"))
        .unwrap_or(GITHUB_URL)
}

impl Render for AboutPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("about")
            .track_focus(&self.focus)
            .size_full()
            .items_center()
            .justify_center()
            .gap_3()
            .px_6()
            .on_key_down(cx.listener(|this, event, window, cx| this.handle_key(event, window, cx)))
            .on_action(cx.listener(|this, _: &crate::CloseTab, window, cx| this.close_window(window, cx)))
            .child(img(app_icon()).w(px(96.0)).h(px(96.0)).object_fit(ObjectFit::Contain).rounded_lg())
            .child(Label::new(APP_NAME).text_xl())
            .child(Label::new(TAGLINE).text_sm())
            .child(Label::new(format!("Version {VERSION}")).text_sm())
            .child(Link::new("github-link").href(GITHUB_URL).child(github_label()))
            .child(
                v_flex()
                    .mt_2()
                    .items_center()
                    .gap_1()
                    .child(Label::new("Uses libghostty by Mitchell Hashimoto and Ghostty contributors.").text_xs())
                    .child(Label::new("Uses GPUI Kit and GPUI Fast, built on GPUI by Zed Industries, Inc.").text_xs())
                    .child(
                        h_flex().child(
                            Link::new("notice-link")
                                .href(format!("{GITHUB_URL}/blob/main/NOTICE"))
                                .child("Acknowledgments"),
                        ),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{GITHUB_URL, VERSION, github_label};

    #[test]
    fn version_comes_from_cargo() {
        assert!(!VERSION.is_empty());
        assert!(VERSION.chars().next().is_some_and(|c| c.is_ascii_digit()));
    }

    #[test]
    fn linux_desktop_appimage_version_matches_cargo() {
        let desktop = include_str!("../packaging/linux/ursa.desktop");
        let expected = format!("X-AppImage-Version={VERSION}");
        assert!(
            desktop.lines().any(|line| line == expected),
            "Shelly reads X-AppImage-Version from the desktop file; keep it in sync with Cargo.toml"
        );
    }

    #[test]
    fn github_link_is_the_repo() {
        assert_eq!(GITHUB_URL, "https://github.com/crwgrant/ursa");
        assert_eq!(github_label(), "github.com/crwgrant/ursa");
    }

    #[test]
    fn notice_attributes_libghostty_and_gpui() {
        let notice = include_str!("../NOTICE");
        assert!(notice.contains("Mitchell Hashimoto, Ghostty contributors"));
        assert!(notice.contains("Uzair Aftab, Leah Amelia Chen"));
        assert!(notice.contains("Zed Industries, Inc."));
        assert!(notice.contains("licenses/ghostty.txt"));
        assert!(notice.contains("licenses/gpui.txt"));
    }
}
