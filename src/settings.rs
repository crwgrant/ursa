use gpui::{
    App, Bounds, Context, Corner, FocusHandle, Global, InteractiveElement, IntoElement, KeyDownEvent, MouseButton,
    MouseDownEvent, ParentElement, Pixels, Render, Styled, TitlebarOptions, Window, WindowBounds, WindowHandle, WindowOptions,
    anchored, div, prelude::*, px, rgb, size,
};

use crate::{config, notify, theme};

#[derive(Default)]
struct SettingsUi {
    window: Option<WindowHandle<SettingsPage>>,
}

impl Global for SettingsUi {}

pub struct SettingsPage {
    focus: FocusHandle,
    font_menu: Option<gpui::Point<Pixels>>,
}

impl SettingsPage {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window);
        cx.observe_global::<config::AppSettings>(|_, cx| cx.notify()).detach();
        Self { focus, font_menu: None }
    }

    fn toggle_font_menu(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        if self.font_menu.is_some() {
            self.font_menu = None;
        } else {
            self.font_menu = Some(event.position);
        }
        cx.notify();
        cx.stop_propagation();
    }

    fn dismiss_font_menu(&mut self, cx: &mut Context<Self>) {
        if self.font_menu.take().is_some() {
            cx.notify();
        }
    }

    fn select_font(&mut self, family: String, cx: &mut Context<Self>) {
        config::update(cx, |config| {
            config.font_family = Some(family);
        });
        self.font_menu = None;
        cx.notify();
    }

    fn handle_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" && self.font_menu.take().is_some() {
            cx.stop_propagation();
            cx.notify();
        }
    }
}

pub fn open(cx: &mut App) {
    if let Some(handle) = cx.try_global::<SettingsUi>().and_then(|ui| ui.window) {
        if handle.update(cx, |_, window, _cx| window.activate_window()).is_ok() {
            return;
        }
    }

    match cx.open_window(window_options(cx), |window, cx| cx.new(|cx| SettingsPage::new(window, cx))) {
        Ok(handle) => {
            let _ = handle.update(cx, |_, window, cx| {
                window.on_window_should_close(cx, |_, cx| {
                    if cx.has_global::<SettingsUi>() {
                        cx.global_mut::<SettingsUi>().window = None;
                    }
                    true
                });
            });
            cx.default_global::<SettingsUi>().window = Some(handle);
        }
        Err(error) => notify::show(cx, format!("Couldn't open settings: {error}")),
    }
}

fn window_options(cx: &App) -> WindowOptions {
    let bounds = Bounds::centered(None, size(px(520.0), px(440.0)), cx);
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some("Settings".into()),
            appears_transparent: false,
            traffic_light_position: None,
        }),
        focus: true,
        show: true,
        kind: gpui::WindowKind::Normal,
        is_movable: true,
        display_id: None,
        window_min_size: Some(size(px(420.0), px(360.0))),
        window_background: gpui::WindowBackgroundAppearance::Opaque,
        app_id: Some(crate::APP_ID.into()),
        is_resizable: true,
        is_minimizable: true,
        window_decorations: None,
        tabbing_identifier: Some("Ghostterm Settings".into()),
    }
}

impl Render for SettingsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let config = config::current(cx);
        let family = config.resolved_font_family();
        let path = config::display_path(cx);
        let error = config::load_error(cx);
        let font_menu = self.font_menu;
        let font_choices = font_menu.map(|_| {
            let installed = cx.text_system().all_font_names();
            config::font_choices(&family, &installed)
        });

        div()
            .id("settings")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(theme::WINDOW))
            .text_color(rgb(theme::TEXT))
            .font_family(theme::UI_FONT_FAMILY)
            .on_key_down(cx.listener(|this, event, _window, cx| this.handle_key(event, cx)))
            .child(
                div()
                    .flex_1()
                    .p_5()
                    .gap_5()
                    .flex()
                    .flex_col()
                    .child(section_label("Font"))
                    .child(font_family_row(family.clone(), cx))
                    .child(setting_row(
                        "Size",
                        format!("{:.0} pt", config.font_size),
                        Some(("font-size-dec", "−", |cx| {
                            config::update(cx, |config| {
                                config.font_size = (config.font_size - config::FONT_SIZE_STEP).max(config::FONT_SIZE_MIN);
                            });
                        })),
                        Some(("font-size-inc", "+", |cx| {
                            config::update(cx, |config| {
                                config.font_size = (config.font_size + config::FONT_SIZE_STEP).min(config::FONT_SIZE_MAX);
                            });
                        })),
                    ))
                    .child(section_label("Terminal"))
                    .child(setting_row(
                        "Scrollback",
                        format!("{} lines", config.scrollback_lines),
                        Some(("scrollback-dec", "−", |cx| {
                            config::update(cx, |config| {
                                config.scrollback_lines = config
                                    .scrollback_lines
                                    .saturating_sub(config::SCROLLBACK_STEP)
                                    .max(config::SCROLLBACK_MIN);
                            });
                        })),
                        Some(("scrollback-inc", "+", |cx| {
                            config::update(cx, |config| {
                                config.scrollback_lines = config
                                    .scrollback_lines
                                    .saturating_add(config::SCROLLBACK_STEP)
                                    .min(config::SCROLLBACK_MAX);
                            });
                        })),
                    ))
                    .child(section_label("Config file"))
                    .child(div().text_xs().text_color(rgb(theme::TEXT_DIM)).child(path))
                    .children(error.map(|message| {
                        div()
                            .text_xs()
                            .text_color(rgb(theme::ACCENT))
                            .child(format!("Could not reload file: {message}"))
                    }))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(text_button("open-config", "Open File", |cx| {
                                if let Some(path) = config::ensure_file(cx) {
                                    cx.open_with_system(&path);
                                }
                            }))
                            .child(text_button("reveal-config", reveal_label(), |cx| {
                                if let Some(path) = config::ensure_file(cx) {
                                    cx.reveal_path(&path);
                                }
                            }))
                            .child(text_button("reload-config", "Reload", |cx| match config::reload(cx) {
                                Ok(()) => notify::show(cx, "Reloaded settings"),
                                Err(error) => notify::show(cx, format!("Config file error: {error}")),
                            })),
                    )
                    .child(text_button("reset-config", "Reset to Defaults", |cx| {
                        config::reset(cx);
                        notify::show(cx, "Settings reset");
                    })),
            )
            .child(
                div()
                    .px_5()
                    .py_3()
                    .border_t_1()
                    .border_color(rgb(theme::SIDEBAR_BORDER))
                    .text_xs()
                    .text_color(rgb(theme::TEXT_DIM))
                    .child("Changes apply immediately and are written to the config file."),
            )
            .children(
                font_menu
                    .zip(font_choices)
                    .map(|(position, choices)| font_family_menu(position, family, choices, cx)),
            )
    }
}

fn font_family_row(family: String, cx: &mut Context<SettingsPage>) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .child(div().text_sm().child("Family"))
        .child(
            div()
                .id("font-family")
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .min_w(px(180.0))
                .h(px(26.0))
                .px_2()
                .rounded_md()
                .bg(rgb(theme::BUTTON))
                .border_1()
                .border_color(rgb(theme::SIDEBAR_BORDER))
                .cursor_pointer()
                .hover(|style| style.bg(rgb(theme::TAB_HOVER)))
                .child(div().flex_1().min_w_0().text_sm().text_ellipsis().child(family))
                .child(div().text_xs().text_color(rgb(theme::TEXT_DIM)).child("▾"))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, event, _window, cx| this.toggle_font_menu(event, cx)),
                ),
        )
}

fn font_family_menu(
    position: gpui::Point<Pixels>,
    selected: String,
    choices: Vec<String>,
    cx: &mut Context<SettingsPage>,
) -> impl IntoElement {
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .occlude()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _event, _window, cx| {
                this.dismiss_font_menu(cx);
                cx.stop_propagation();
            }),
        )
        .child(
            anchored().position(position).anchor(Corner::TopLeft).child(
                div()
                    .id("font-family-menu")
                    .occlude()
                    .min_w(px(180.0))
                    .max_w(px(280.0))
                    .max_h(px(240.0))
                    .overflow_y_scroll()
                    .py_1()
                    .rounded_md()
                    .bg(rgb(theme::TOOLTIP))
                    .border_1()
                    .border_color(rgb(theme::SIDEBAR_BORDER))
                    .shadow_md()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .children(choices.into_iter().enumerate().map(|(index, family)| {
                        let selected = family.eq_ignore_ascii_case(&selected);
                        font_menu_item(index, family, selected, cx)
                    })),
            ),
        )
}

fn font_menu_item(index: usize, family: String, selected: bool, cx: &mut Context<SettingsPage>) -> impl IntoElement {
    let on_select = family.clone();
    div()
        .id(("font-choice", index))
        .w_full()
        .px_3()
        .py_1()
        .text_sm()
        .text_color(if selected { rgb(theme::ACCENT) } else { rgb(theme::TEXT) })
        .bg(if selected {
            rgb(theme::TAB_ACTIVE)
        } else {
            rgb(theme::TOOLTIP)
        })
        .cursor_pointer()
        .hover(|style| style.bg(rgb(theme::TAB_HOVER)))
        .child(family)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _event, _window, cx| {
                cx.stop_propagation();
                this.select_font(on_select.clone(), cx);
            }),
        )
}

fn section_label(label: &'static str) -> impl IntoElement {
    div()
        .text_xs()
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(rgb(theme::TEXT_DIM))
        .child(label)
}

fn setting_row(
    label: &'static str,
    value: String,
    left: Option<(&'static str, &'static str, fn(&mut App))>,
    right: Option<(&'static str, &'static str, fn(&mut App))>,
) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .child(div().text_sm().child(label))
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .children(left.map(|(id, glyph, action)| stepper_button(id, glyph, action)))
                .child(div().min_w(px(120.0)).text_sm().text_color(rgb(theme::TEXT)).child(value))
                .children(right.map(|(id, glyph, action)| stepper_button(id, glyph, action))),
        )
}

fn stepper_button(id: &'static str, label: &'static str, on_click: fn(&mut App)) -> impl IntoElement {
    div()
        .id(id)
        .h(px(22.0))
        .w(px(22.0))
        .rounded_md()
        .flex()
        .items_center()
        .justify_center()
        .bg(rgb(theme::BUTTON))
        .text_color(rgb(theme::TEXT))
        .text_sm()
        .cursor_pointer()
        .hover(|style| style.bg(rgb(theme::TAB_HOVER)))
        .child(label)
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            on_click(cx);
            cx.stop_propagation();
        })
}

fn text_button(id: &'static str, label: &'static str, on_click: fn(&mut App)) -> impl IntoElement {
    div()
        .id(id)
        .px_3()
        .py_1()
        .rounded_md()
        .bg(rgb(theme::BUTTON))
        .text_xs()
        .text_color(rgb(theme::TEXT))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(theme::TAB_HOVER)))
        .child(label)
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            on_click(cx);
            cx.stop_propagation();
        })
}

fn reveal_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "Reveal in Finder"
    } else if cfg!(target_os = "windows") {
        "Show in Explorer"
    } else {
        "Reveal File"
    }
}
