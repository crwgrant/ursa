use std::sync::{Arc, OnceLock};

use gpui::{
    App, Bounds, Context, CursorStyle, DisplayId, FocusHandle, Global, Image, ImageFormat, InteractiveElement, IntoElement,
    KeyDownEvent, MouseButton, ObjectFit, ParentElement, Render, Styled, TitlebarOptions, Window, WindowBounds, WindowHandle,
    WindowOptions, div, img, prelude::*, px, rgb, size,
};

use crate::{notify, theme};

const APP_NAME: &str = "Ursa";
const VERSION: &str = env!("CARGO_PKG_VERSION");
const TAGLINE: &str = env!("CARGO_PKG_DESCRIPTION");
const GITHUB_URL: &str = env!("CARGO_PKG_REPOSITORY");
const ICON_PNG: &[u8] = include_bytes!("../packaging/AppIcon-small.png");

#[derive(Default)]
struct AboutUi {
    window: Option<WindowHandle<AboutPage>>,
}

impl Global for AboutUi {}

pub struct AboutPage {
    focus: FocusHandle,
}

impl AboutPage {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window);
        cx.observe_global::<theme::ThemeCatalog>(|_, cx| cx.notify()).detach();
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
        if handle.update(cx, |_, window, _cx| window.activate_window()).is_ok() {
            return;
        }
    }

    let display_id = host_display_id(cx);
    match cx.open_window(window_options(cx, display_id), |window, cx| cx.new(|cx| AboutPage::new(window, cx))) {
        Ok(handle) => {
            let _ = handle.update(cx, |_, window, cx| {
                window.on_window_should_close(cx, |_, cx| {
                    if cx.has_global::<AboutUi>() {
                        cx.global_mut::<AboutUi>().window = None;
                    }
                    crate::quit_if_last_window(cx);
                    true
                });
            });
            cx.default_global::<AboutUi>().window = Some(handle);
        }
        Err(error) => notify::show(cx, format!("Couldn't open About: {error}")),
    }
}

fn host_display_id(cx: &mut App) -> Option<DisplayId> {
    cx.active_window().and_then(|handle| {
        handle
            .update(cx, |_, window, cx| window.display(cx).map(|display| display.id()))
            .ok()
            .flatten()
    })
}

fn window_options(cx: &App, display_id: Option<DisplayId>) -> WindowOptions {
    let bounds = Bounds::centered(display_id, size(px(380.0), px(400.0)), cx);
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
        window_min_size: Some(size(px(360.0), px(360.0))),
        window_background: gpui::WindowBackgroundAppearance::Opaque,
        app_id: Some(crate::APP_ID.into()),
        is_resizable: false,
        is_minimizable: true,
        window_decorations: None,
        tabbing_identifier: Some("Ursa About".into()),
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
        let colors = theme::colors(cx);
        div()
            .id("about")
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .px_6()
            .bg(rgb(colors.window))
            .text_color(rgb(colors.text))
            .font_family(theme::UI_FONT_FAMILY)
            .on_key_down(cx.listener(|this, event, window, cx| this.handle_key(event, window, cx)))
            .on_action(cx.listener(|this, _: &crate::CloseTab, window, cx| this.close_window(window, cx)))
            .child(
                img(app_icon())
                    .w(px(96.0))
                    .h(px(96.0))
                    .object_fit(ObjectFit::Contain)
                    .rounded_lg(),
            )
            .child(div().text_xl().font_weight(gpui::FontWeight::SEMIBOLD).child(APP_NAME))
            .child(div().text_sm().text_color(rgb(colors.text_dim)).child(TAGLINE))
            .child(div().text_sm().child(format!("Version {VERSION}")))
            .child(
                div()
                    .id("github-link")
                    .text_sm()
                    .text_color(rgb(colors.accent))
                    .cursor(CursorStyle::PointingHand)
                    .hover(|style| style.underline())
                    .child(github_label())
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        cx.open_url(GITHUB_URL);
                        cx.stop_propagation();
                    }),
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
}
