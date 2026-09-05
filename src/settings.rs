use std::time::Duration;

use gpui::{
    Animation, AnimationExt as _, AnyElement, App, Bounds, Context, Corner, CursorStyle, DisplayId, Entity, FocusHandle,
    Focusable, Global, InteractiveElement, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, ParentElement, Pixels, Render,
    Styled, TitlebarOptions, Window, WindowBounds, WindowHandle, WindowOptions, anchored, div, prelude::*, px, rgb, size,
};

use crate::{config, notify, theme};

#[derive(Default)]
struct SettingsUi {
    window: Option<WindowHandle<SettingsPage>>,
}

impl Global for SettingsUi {}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuKind {
    FontFamily,
    FontSize,
    Cursor,
    OnExit,
    SessionSidebar,
    Theme,
}

pub struct SettingsPage {
    focus: FocusHandle,
    open_menu: Option<(MenuKind, gpui::Point<Pixels>)>,
    scrollback: Entity<ScrollbackField>,
}

impl SettingsPage {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window);
        let scrollback = cx.new(|cx| ScrollbackField::new(config::scrollback_lines(cx), window, cx));
        cx.observe_global::<config::AppSettings>(|this, cx| {
            let lines = config::scrollback_lines(cx);
            this.scrollback.update(cx, |field, cx| field.sync_from_config(lines, cx));
            cx.notify();
        })
        .detach();
        cx.observe_global::<theme::ThemeCatalog>(|_, cx| cx.notify()).detach();
        Self {
            focus,
            open_menu: None,
            scrollback,
        }
    }

    fn toggle_menu(&mut self, kind: MenuKind, event: &MouseDownEvent, cx: &mut Context<Self>) {
        if self.open_menu.map(|(open, _)| open) == Some(kind) {
            self.open_menu = None;
        } else {
            self.open_menu = Some((kind, event.position));
        }
        cx.notify();
        cx.stop_propagation();
    }

    fn dismiss_menu(&mut self, cx: &mut Context<Self>) {
        if self.open_menu.take().is_some() {
            cx.notify();
        }
    }

    fn select_font(&mut self, family: String, cx: &mut Context<Self>) {
        config::update(cx, |config| {
            config.font_family = Some(family);
        });
        self.open_menu = None;
        cx.notify();
    }

    fn select_font_size(&mut self, size: f32, cx: &mut Context<Self>) {
        config::update(cx, |config| {
            config.font_size = size;
        });
        self.open_menu = None;
        cx.notify();
    }

    fn select_cursor(&mut self, shape: config::CursorShape, cx: &mut Context<Self>) {
        config::update(cx, |config| {
            config.cursor_shape = shape;
        });
        self.open_menu = None;
        cx.notify();
    }

    fn select_on_exit(&mut self, on_exit: config::OnExit, cx: &mut Context<Self>) {
        config::update(cx, |config| {
            config.on_exit = on_exit;
        });
        self.open_menu = None;
        cx.notify();
    }

    fn select_session_sidebar(&mut self, session_sidebar: config::SessionSidebar, cx: &mut Context<Self>) {
        config::update(cx, |config| {
            config.session_sidebar = session_sidebar;
        });
        self.open_menu = None;
        cx.notify();
    }

    fn select_theme(&mut self, theme: String, cx: &mut Context<Self>) {
        config::update(cx, |config| {
            config.theme = theme;
        });
        self.open_menu = None;
        cx.notify();
    }

    fn handle_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" && self.open_menu.take().is_some() {
            cx.stop_propagation();
            cx.notify();
        }
    }

    fn close_window(&mut self, window: &mut Window, _cx: &mut Context<Self>) {
        window.remove_window();
    }
}

pub fn open(cx: &mut App) {
    if let Some(handle) = cx.try_global::<SettingsUi>().and_then(|ui| ui.window) {
        if handle.update(cx, |_, window, _cx| window.activate_window()).is_ok() {
            return;
        }
    }

    let display_id = host_display_id(cx);
    match cx.open_window(window_options(cx, display_id), |window, cx| cx.new(|cx| SettingsPage::new(window, cx))) {
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

fn host_display_id(cx: &mut App) -> Option<DisplayId> {
    cx.active_window().and_then(|handle| {
        handle
            .update(cx, |_, window, cx| window.display(cx).map(|display| display.id()))
            .ok()
            .flatten()
    })
}

fn window_options(cx: &App, display_id: Option<DisplayId>) -> WindowOptions {
    let bounds = Bounds::centered(display_id, size(px(520.0), px(640.0)), cx);
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
        display_id,
        window_min_size: Some(size(px(420.0), px(600.0))),
        window_background: gpui::WindowBackgroundAppearance::Opaque,
        app_id: Some(crate::APP_ID.into()),
        is_resizable: true,
        is_minimizable: true,
        window_decorations: None,
        tabbing_identifier: Some("Skiff Settings".into()),
    }
}

impl Render for SettingsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let config = config::current(cx);
        let family = config::font_family(cx).to_string();
        let font_size = config.font_size;
        let cursor_shape = config.cursor_shape;
        let on_exit = config.on_exit;
        let session_sidebar = config.session_sidebar;
        let app_theme = config.theme;
        let path = config::display_path(cx);
        let error = config::load_error(cx);
        let theme_error = theme::catalog(cx).error;
        let colors = theme::colors(cx);
        let open_menu = self.open_menu;
        let menu: Option<AnyElement> = open_menu.map(|(kind, position)| match kind {
            MenuKind::FontFamily => {
                let installed = cx.text_system().all_font_names();
                let choices = config::font_choices(&family, &installed);
                let items: Vec<_> = choices
                    .into_iter()
                    .enumerate()
                    .map(|(index, name)| {
                        let selected = name.eq_ignore_ascii_case(&family);
                        let on_select = name.clone();
                        dropdown_item(("font-choice", index).into(), name, selected, cx, move |this, cx| {
                            this.select_font(on_select.clone(), cx);
                        })
                    })
                    .collect();
                dropdown_overlay("font-family-menu", position, items, cx).into_any_element()
            }
            MenuKind::FontSize => {
                let items: Vec<_> = config::font_size_choices(font_size)
                    .into_iter()
                    .enumerate()
                    .map(|(index, size)| {
                        let selected = (size - font_size).abs() < 0.001;
                        dropdown_item(("font-size", index).into(), format_font_size(size), selected, cx, move |this, cx| {
                            this.select_font_size(size, cx);
                        })
                    })
                    .collect();
                dropdown_overlay("font-size-menu", position, items, cx).into_any_element()
            }
            MenuKind::Cursor => {
                let items: Vec<_> = config::CursorShape::all()
                    .into_iter()
                    .enumerate()
                    .map(|(index, shape)| {
                        dropdown_item(
                            ("cursor-choice", index).into(),
                            shape.label().to_string(),
                            shape == cursor_shape,
                            cx,
                            move |this, cx| this.select_cursor(shape, cx),
                        )
                    })
                    .collect();
                dropdown_overlay("cursor-menu", position, items, cx).into_any_element()
            }
            MenuKind::OnExit => {
                let items: Vec<_> = config::OnExit::all()
                    .into_iter()
                    .enumerate()
                    .map(|(index, option)| {
                        dropdown_item(
                            ("on-exit-choice", index).into(),
                            option.label().to_string(),
                            option == on_exit,
                            cx,
                            move |this, cx| this.select_on_exit(option, cx),
                        )
                    })
                    .collect();
                dropdown_overlay("on-exit-menu", position, items, cx).into_any_element()
            }
            MenuKind::SessionSidebar => {
                let items: Vec<_> = config::SessionSidebar::all()
                    .into_iter()
                    .enumerate()
                    .map(|(index, option)| {
                        dropdown_item(
                            ("session-sidebar-choice", index).into(),
                            option.label().to_string(),
                            option == session_sidebar,
                            cx,
                            move |this, cx| this.select_session_sidebar(option, cx),
                        )
                    })
                    .collect();
                dropdown_overlay("session-sidebar-menu", position, items, cx).into_any_element()
            }
            MenuKind::Theme => {
                let items: Vec<_> = theme::choices(cx)
                    .into_iter()
                    .enumerate()
                    .map(|(index, (id, label))| {
                        let selected = id == app_theme;
                        dropdown_item(("theme-choice", index).into(), label, selected, cx, move |this, cx| {
                            this.select_theme(id.clone(), cx);
                        })
                    })
                    .collect();
                dropdown_overlay("theme-menu", position, items, cx).into_any_element()
            }
        });

        div()
            .id("settings")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(colors.window))
            .text_color(rgb(colors.text))
            .font_family(theme::UI_FONT_FAMILY)
            .on_key_down(cx.listener(|this, event, _window, cx| this.handle_key(event, cx)))
            .on_action(cx.listener(|this, _: &crate::CloseTab, window, cx| this.close_window(window, cx)))
            .child(
                div()
                    .flex_1()
                    .p_5()
                    .gap_5()
                    .flex()
                    .flex_col()
                    .child(section_label("Appearance", colors))
                    .child(theme_row(app_theme, cx))
                    .child(section_label("Font", colors))
                    .child(font_family_row(family.clone(), cx))
                    .child(font_size_row(font_size, cx))
                    .child(section_label("Terminal", colors))
                    .child(cursor_row(cursor_shape, cx))
                    .child(on_exit_row(on_exit, cx))
                    .child(session_sidebar_row(session_sidebar, cx))
                    .child(scrollback_row(self.scrollback.clone(), colors))
                    .child(section_label("Config file", colors))
                    .child(div().text_xs().text_color(rgb(colors.text_dim)).child(path))
                    .children(error.map(|message| {
                        div()
                            .text_xs()
                            .text_color(rgb(colors.accent))
                            .child(format!("Could not reload file: {message}"))
                    }))
                    .children(theme_error.map(|message| {
                        div()
                            .text_xs()
                            .text_color(rgb(colors.accent))
                            .child(format!("Could not load themes file: {message}"))
                    }))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(text_button("open-config", "Open File", colors, |cx| {
                                if let Some(path) = config::ensure_file(cx) {
                                    cx.open_with_system(&path);
                                }
                            }))
                            .child(text_button("reveal-config", reveal_label(), colors, |cx| {
                                if let Some(path) = config::ensure_file(cx) {
                                    cx.reveal_path(&path);
                                }
                            }))
                            .child(text_button("reload-config", "Reload", colors, |cx| match config::reload(cx) {
                                Ok(()) => notify::show(cx, "Reloaded settings"),
                                Err(error) => notify::show(cx, format!("Config file error: {error}")),
                            })),
                    )
                    .child(text_button("reset-config", "Reset to Defaults", colors, |cx| {
                        config::reset(cx);
                        crate::reset_workspace_sidebars(cx);
                        notify::show(cx, "Settings reset");
                    })),
            )
            .child(
                div()
                    .px_5()
                    .py_3()
                    .border_t_1()
                    .border_color(rgb(colors.sidebar_border))
                    .text_xs()
                    .text_color(rgb(colors.text_dim))
                    .child(
                        "Theme, font, cursor, and exit behavior apply immediately. Scrollback saves when the field loses focus.",
                    ),
            )
            .children(menu)
    }
}

fn font_family_row(family: String, cx: &mut Context<SettingsPage>) -> impl IntoElement {
    dropdown_row("font-family", "Family", family, MenuKind::FontFamily, cx)
}

fn font_size_row(size: f32, cx: &mut Context<SettingsPage>) -> impl IntoElement {
    dropdown_row("font-size", "Size", format_font_size(size), MenuKind::FontSize, cx)
}

fn cursor_row(shape: config::CursorShape, cx: &mut Context<SettingsPage>) -> impl IntoElement {
    dropdown_row("cursor-shape", "Cursor", shape.label().to_string(), MenuKind::Cursor, cx)
}

fn on_exit_row(on_exit: config::OnExit, cx: &mut Context<SettingsPage>) -> impl IntoElement {
    dropdown_row("on-exit", "Sessions", on_exit.label().to_string(), MenuKind::OnExit, cx)
}

fn session_sidebar_row(session_sidebar: config::SessionSidebar, cx: &mut Context<SettingsPage>) -> impl IntoElement {
    dropdown_row(
        "session-sidebar",
        "Session sidebar",
        session_sidebar.label().to_string(),
        MenuKind::SessionSidebar,
        cx,
    )
}

fn theme_row(app_theme: String, cx: &mut Context<SettingsPage>) -> impl IntoElement {
    dropdown_row("app-theme", "Theme", theme::label_for(&app_theme, cx), MenuKind::Theme, cx)
}

fn dropdown_row(
    id: &'static str,
    label: &'static str,
    value: String,
    kind: MenuKind,
    cx: &mut Context<SettingsPage>,
) -> impl IntoElement {
    let colors = theme::colors(cx);
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .child(div().text_sm().child(label))
        .child(
            div()
                .id(id)
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .min_w(px(180.0))
                .h(px(26.0))
                .px_2()
                .rounded_md()
                .bg(rgb(colors.button))
                .border_1()
                .border_color(rgb(colors.sidebar_border))
                .cursor_pointer()
                .hover(move |style| style.bg(rgb(colors.tab_hover)))
                .child(div().flex_1().min_w_0().text_sm().text_ellipsis().child(value))
                .child(div().text_xs().text_color(rgb(colors.text_dim)).child("▾"))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event, _window, cx| this.toggle_menu(kind, event, cx)),
                ),
        )
}

fn dropdown_overlay(
    id: &'static str,
    position: gpui::Point<Pixels>,
    items: Vec<impl IntoElement>,
    cx: &mut Context<SettingsPage>,
) -> impl IntoElement {
    let colors = theme::colors(cx);
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .occlude()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _event, _window, cx| {
                this.dismiss_menu(cx);
                cx.stop_propagation();
            }),
        )
        .child(
            anchored().position(position).anchor(Corner::TopLeft).child(
                div()
                    .id(id)
                    .occlude()
                    .min_w(px(180.0))
                    .max_w(px(280.0))
                    .max_h(px(240.0))
                    .overflow_y_scroll()
                    .py_1()
                    .rounded_md()
                    .bg(rgb(colors.tooltip))
                    .border_1()
                    .border_color(rgb(colors.sidebar_border))
                    .shadow_md()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .children(items),
            ),
        )
}

fn dropdown_item(
    id: gpui::ElementId,
    label: String,
    selected: bool,
    cx: &mut Context<SettingsPage>,
    on_click: impl Fn(&mut SettingsPage, &mut Context<SettingsPage>) + 'static,
) -> impl IntoElement {
    let colors = theme::colors(cx);
    let on_click: std::rc::Rc<dyn Fn(&mut SettingsPage, &mut Context<SettingsPage>)> = std::rc::Rc::new(on_click);
    div()
        .id(id)
        .w_full()
        .px_3()
        .py_1()
        .text_sm()
        .text_color(if selected { rgb(colors.accent) } else { rgb(colors.text) })
        .bg(if selected {
            rgb(colors.tab_active)
        } else {
            rgb(colors.tooltip)
        })
        .cursor_pointer()
        .hover(move |style| style.bg(rgb(colors.tab_hover)))
        .child(label)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _event, _window, cx| {
                cx.stop_propagation();
                (on_click)(this, cx);
            }),
        )
}

fn format_font_size(size: f32) -> String {
    if (size - size.round()).abs() < 0.001 {
        format!("{:.0} pt", size.round())
    } else {
        format!("{size} pt")
    }
}

struct ScrollbackField {
    focus: FocusHandle,
    text: String,
    committed: String,
    selected: bool,
}

impl ScrollbackField {
    fn new(lines: u32, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let text = lines.to_string();
        let focus = cx.focus_handle();
        cx.on_blur(&focus, window, |this, _window, cx| this.commit(cx)).detach();
        Self {
            focus,
            committed: text.clone(),
            text,
            selected: false,
        }
    }

    fn sync_from_config(&mut self, lines: u32, cx: &mut Context<Self>) {
        if self.text != self.committed {
            return;
        }
        let next = lines.to_string();
        if self.text != next {
            self.text = next.clone();
            self.committed = next;
            cx.notify();
        }
    }

    fn commit(&mut self, cx: &mut Context<Self>) {
        if let Some(lines) = config::parse_scrollback(&self.text) {
            self.text = lines.to_string();
            if self.committed != self.text {
                self.committed = self.text.clone();
                config::update(cx, |config| config.scrollback_lines = lines);
            }
        } else {
            self.text = self.committed.clone();
        }
        self.selected = false;
        cx.notify();
    }

    fn revert(&mut self, cx: &mut Context<Self>) {
        self.text = self.committed.clone();
        self.selected = false;
        cx.notify();
    }

    fn insert_digits(&mut self, digits: &str, cx: &mut Context<Self>) {
        if digits.is_empty() {
            return;
        }
        if self.selected {
            self.text = digits.to_string();
            self.selected = false;
        } else {
            self.text.push_str(digits);
        }
        cx.notify();
    }

    fn handle_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let modifiers = &event.keystroke.modifiers;
        let key = event.keystroke.key.as_str();
        if modifiers.platform && key == "v" {
            let digits = cx
                .read_from_clipboard()
                .and_then(|item| item.text())
                .map(|text| text.chars().filter(|ch| ch.is_ascii_digit()).collect::<String>())
                .unwrap_or_default();
            self.insert_digits(&digits, cx);
            cx.stop_propagation();
            return;
        }
        if modifiers.platform && key == "c" {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(self.text.clone()));
            cx.stop_propagation();
            return;
        }
        if modifiers.platform || modifiers.control || modifiers.alt {
            return;
        }
        if key == "escape" {
            self.revert(cx);
            cx.stop_propagation();
            return;
        }
        if key == "enter" {
            self.commit(cx);
            cx.stop_propagation();
            return;
        }
        if key == "backspace" {
            if self.selected {
                self.text.clear();
                self.selected = false;
            } else {
                self.text.pop();
            }
            cx.notify();
            cx.stop_propagation();
            return;
        }
        let digit = event
            .keystroke
            .key_char
            .as_deref()
            .filter(|text| text.len() == 1 && text.chars().all(|ch| ch.is_ascii_digit()))
            .or_else(|| (key.len() == 1 && key.chars().all(|ch| ch.is_ascii_digit())).then_some(key));
        if let Some(digit) = digit {
            self.insert_digits(digit, cx);
            cx.stop_propagation();
        }
    }
}

impl Focusable for ScrollbackField {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ScrollbackField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus.is_focused(window);
        let selected = focused && self.selected;
        let colors = theme::colors(cx);
        div()
            .id("scrollback-field")
            .track_focus(&self.focus)
            .flex()
            .items_center()
            .min_w(px(180.0))
            .h(px(26.0))
            .px_2()
            .rounded_md()
            .bg(if selected {
                rgb(colors.tab_active)
            } else {
                rgb(colors.button)
            })
            .border_1()
            .border_color(if focused {
                rgb(colors.accent)
            } else {
                rgb(colors.sidebar_border)
            })
            .cursor(CursorStyle::IBeam)
            .on_key_down(cx.listener(|this, event, _window, cx| this.handle_key(event, cx)))
            .on_action(|_: &crate::CloseTab, window, _cx| window.remove_window())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _event, window, cx| {
                    this.focus.focus(window);
                    this.selected = true;
                    cx.notify();
                    cx.stop_propagation();
                }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .flex_1()
                    .min_w_0()
                    .child(div().text_sm().child(self.text.clone()))
                    .children(focused.then(|| {
                        div()
                            .ml(px(1.0))
                            .w(px(1.5))
                            .h(px(14.0))
                            .bg(rgb(colors.text))
                            .rounded_sm()
                            .with_animation(
                                "scrollback-caret",
                                Animation::new(Duration::from_millis(1000)).repeat(),
                                |caret, delta| caret.opacity(if delta < 0.5 { 1.0 } else { 0.0 }),
                            )
                    })),
            )
    }
}

fn scrollback_row(field: Entity<ScrollbackField>, colors: theme::Colors) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .child(div().text_sm().child("Scrollback"))
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(field)
                .child(div().text_sm().text_color(rgb(colors.text_dim)).child("lines")),
        )
}

fn section_label(label: &'static str, colors: theme::Colors) -> impl IntoElement {
    div()
        .text_xs()
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(rgb(colors.text_dim))
        .child(label)
}

fn text_button(id: &'static str, label: &'static str, colors: theme::Colors, on_click: fn(&mut App)) -> impl IntoElement {
    div()
        .id(id)
        .px_3()
        .py_1()
        .rounded_md()
        .bg(rgb(colors.button))
        .text_xs()
        .text_color(rgb(colors.text))
        .cursor_pointer()
        .hover(move |style| style.bg(rgb(colors.tab_hover)))
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
