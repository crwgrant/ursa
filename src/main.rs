#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod frame;
mod input;
mod notify;
mod pty;
mod session;
mod settings;
mod theme;

use gpui::{
    AnyView, App, Application, Bounds, Context, KeyBinding, Menu, MenuItem, MouseButton, SharedString, TitlebarOptions, Window,
    WindowBounds, WindowOptions, actions, div, point, prelude::*, px, rgb, size,
};
use session::{Session, SessionEvent};

actions!(ghostterm, [Quit, NewWindow, NewTab, CloseTab, Copy, Paste, ClearScreen, OpenSettings]);

pub(crate) const APP_ID: &str = "com.crwgrant.ghostterm";
const WINDOW_WIDTH: f32 = 1100.0;
const WINDOW_HEIGHT: f32 = 720.0;
const NEW_WINDOW_OFFSET: f32 = 28.0;

struct Workspace {
    tabs: Vec<gpui::Entity<Session>>,
    active: usize,
}

impl Workspace {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let first = cx.new(|cx| Session::spawn(0, window, cx));
        let workspace = Self {
            tabs: vec![first],
            active: 0,
        };
        workspace.subscribe_session(0, window, cx);
        cx.observe_global::<notify::Notifications>(|_, cx| cx.notify()).detach();
        cx.observe_global::<config::AppSettings>(|this, cx| {
            this.apply_config(cx);
            cx.notify();
        })
        .detach();
        cx.observe_global::<theme::ThemeCatalog>(|this, cx| {
            this.apply_config(cx);
            cx.notify();
        })
        .detach();
        workspace
    }

    fn subscribe_session(&self, index: usize, window: &Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(index).cloned() else {
            return;
        };
        cx.subscribe_in(&tab, window, |this, session, event: &SessionEvent, window, cx| {
            if matches!(event, SessionEvent::Exited) {
                if let Some(index) = this.tabs.iter().position(|tab| tab == session) {
                    this.close_tab(index, window, cx);
                }
            }
        })
        .detach();
    }

    fn add_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let index = self.tabs.len();
        let tab = cx.new(|cx| Session::spawn(index, window, cx));
        self.tabs.push(tab);
        self.active = index;
        self.subscribe_session(index, window, cx);
        self.focus_active(window, cx);
        cx.notify();
    }

    fn select_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index < self.tabs.len() {
            self.active = index;
            self.focus_active(window, cx);
            cx.notify();
        }
    }

    fn close_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }
        if self.tabs.len() == 1 {
            window.remove_window();
            return;
        }

        self.tabs.remove(index);
        if self.active == index {
            self.active = index.saturating_sub(1);
        } else if self.active > index {
            self.active -= 1;
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    fn close_active_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_tab(self.active, window, cx);
    }

    fn focus_active(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get(self.active) {
            tab.update(cx, |session, _cx| session.focus(window));
        }
    }

    fn copy_active(&self, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get(self.active) {
            tab.update(cx, |session, cx| session.copy_selection(cx));
        }
    }

    fn paste_active(&self, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get(self.active) {
            tab.update(cx, |session, cx| session.paste_clipboard(cx));
        }
    }

    fn clear_active(&self, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get(self.active) {
            tab.update(cx, |session, _cx| session.clear_screen());
        }
    }

    fn apply_config(&self, cx: &mut Context<Self>) {
        for tab in &self.tabs {
            tab.update(cx, |session, cx| session.apply_config(cx));
        }
    }
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.active;
        let tabs: Vec<(usize, SharedString, bool)> = self
            .tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                let title = tab.read(cx).title.clone();
                (index, SharedString::from(title), index == active)
            })
            .collect();

        let colors = theme::colors(cx);
        div()
            .relative()
            .flex()
            .size_full()
            .bg(rgb(colors.window))
            .text_color(rgb(colors.text))
            .font_family(theme::UI_FONT_FAMILY)
            .on_action(cx.listener(|this, _: &NewTab, window, cx| this.add_tab(window, cx)))
            .on_action(cx.listener(|this, _: &CloseTab, window, cx| this.close_active_tab(window, cx)))
            .on_action(cx.listener(|this, _: &Copy, _window, cx| this.copy_active(cx)))
            .on_action(cx.listener(|this, _: &Paste, _window, cx| this.paste_active(cx)))
            .on_action(cx.listener(|this, _: &ClearScreen, _window, cx| this.clear_active(cx)))
            .on_action(|_: &OpenSettings, _window, cx| crate::settings::open(cx))
            .child(self.render_sidebar(&tabs, cx))
            .child(self.render_terminal())
            .child(notify::overlay(cx))
    }
}

impl Workspace {
    fn render_sidebar(&self, tabs: &[(usize, SharedString, bool)], cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        div()
            .w(px(theme::SIDEBAR_WIDTH))
            .h_full()
            .bg(rgb(colors.sidebar))
            .border_r_1()
            .border_color(rgb(colors.sidebar_border))
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .py_3()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgb(colors.text_dim))
                            .child("SESSIONS"),
                    )
                    .child(new_tab_button(cx)),
            )
            .child(
                div()
                    .id("tab-list")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .px_2()
                    .gap_1()
                    .overflow_y_scroll()
                    .children(
                        tabs.iter()
                            .map(|(index, title, selected)| tab_row(*index, title.clone(), *selected, cx)),
                    ),
            )
            .child(settings_button(cx))
    }

    fn render_terminal(&self) -> impl IntoElement {
        div().flex_1().h_full().min_w_0().child(self.tabs[self.active].clone())
    }
}

fn settings_button(cx: &App) -> impl IntoElement {
    let colors = theme::colors(cx);
    div()
        .px_2()
        .py_2()
        .border_t_1()
        .border_color(rgb(colors.sidebar_border))
        .flex_shrink_0()
        .child(
            div()
                .id("open-settings")
                .w_full()
                .rounded_md()
                .px_3()
                .py_2()
                .flex()
                .items_center()
                .gap_2()
                .cursor_pointer()
                .hover(move |style| style.bg(rgb(colors.tab_hover)))
                .tooltip(action_tooltip("Settings", settings_shortcut()))
                .child(div().w(px(6.0)).h(px(6.0)).rounded_full().bg(rgb(colors.text_dim)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_sm()
                        .text_color(rgb(colors.text_dim))
                        .child("Settings"),
                )
                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                    crate::settings::open(cx);
                    cx.stop_propagation();
                }),
        )
}

fn new_tab_button(cx: &mut Context<Workspace>) -> impl IntoElement {
    let colors = theme::colors(cx);
    div()
        .id("new-tab")
        .h(px(22.0))
        .w(px(22.0))
        .rounded_md()
        .flex()
        .items_center()
        .justify_center()
        .bg(rgb(colors.button))
        .text_color(rgb(colors.text))
        .text_sm()
        .cursor_pointer()
        .hover(move |style| style.bg(rgb(colors.tab_hover)))
        .tooltip(action_tooltip("New Tab", new_tab_shortcut()))
        .child("+")
        .on_mouse_down(MouseButton::Left, cx.listener(|this, _event, window, cx| this.add_tab(window, cx)))
}

fn close_tab_button(index: usize, cx: &mut Context<Workspace>) -> impl IntoElement {
    let colors = theme::colors(cx);
    div()
        .id(("tab-close", index))
        .h(px(18.0))
        .w(px(18.0))
        .rounded_md()
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .text_xs()
        .text_color(rgb(colors.text_dim))
        .cursor_pointer()
        .hover(move |style| style.bg(rgb(colors.button)).text_color(rgb(colors.text)))
        .tooltip(action_tooltip("Close Tab", close_tab_shortcut()))
        .child("×")
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _event, window, cx| {
                cx.stop_propagation();
                this.close_tab(index, window, cx);
            }),
        )
}

fn tab_row(index: usize, title: SharedString, selected: bool, cx: &mut Context<Workspace>) -> impl IntoElement {
    let colors = theme::colors(cx);
    let background = if selected {
        rgb(colors.tab_active)
    } else {
        rgb(colors.sidebar)
    };

    div()
        .id(("tab", index))
        .w_full()
        .rounded_md()
        .px_3()
        .py_2()
        .bg(background)
        .cursor_pointer()
        .hover(move |style| if selected { style } else { style.bg(rgb(colors.tab_hover)) })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _event, window, cx| this.select_tab(index, window, cx)),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .w_full()
                .child(div().w(px(6.0)).h(px(6.0)).rounded_full().bg(if selected {
                    rgb(colors.accent)
                } else {
                    rgb(colors.text_dim)
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_sm()
                        .text_ellipsis()
                        .text_color(if selected { rgb(colors.text) } else { rgb(colors.text_dim) })
                        .child(title),
                )
                .child(close_tab_button(index, cx)),
        )
}

struct ActionTooltip {
    label: SharedString,
    shortcut: SharedString,
}

fn action_tooltip(
    label: impl Into<SharedString>,
    shortcut: impl Into<SharedString>,
) -> impl Fn(&mut Window, &mut App) -> AnyView {
    let label = label.into();
    let shortcut = shortcut.into();
    move |_, cx| {
        cx.new(|_| ActionTooltip {
            label: label.clone(),
            shortcut: shortcut.clone(),
        })
        .into()
    }
}

impl Render for ActionTooltip {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        div()
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(rgb(colors.tooltip))
            .border_1()
            .border_color(rgb(colors.sidebar_border))
            .shadow_md()
            .text_xs()
            .child(div().text_color(rgb(colors.text)).child(self.label.clone()))
            .child(div().text_color(rgb(colors.text_dim)).child(self.shortcut.clone()))
    }
}

fn new_tab_shortcut() -> &'static str {
    if cfg!(target_os = "macos") { "⌘T" } else { "Ctrl+Shift+T" }
}

fn close_tab_shortcut() -> &'static str {
    if cfg!(target_os = "macos") { "⌘W" } else { "Ctrl+Shift+W" }
}

fn settings_shortcut() -> &'static str {
    if cfg!(target_os = "macos") { "⌘," } else { "Ctrl+," }
}

fn main() {
    let app = Application::new();
    app.on_reopen(|cx| {
        if cx.windows().is_empty() {
            open_workspace_window(cx);
        }
    });
    app.run(|cx: &mut App| {
        config::init(cx);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.on_action(|_: &NewWindow, cx| {
            open_workspace_window(cx);
        });
        cx.on_action(|_: &OpenSettings, cx| settings::open(cx));
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-n", NewWindow, None),
            KeyBinding::new("cmd-t", NewTab, None),
            KeyBinding::new("cmd-w", CloseTab, None),
            KeyBinding::new("cmd-c", Copy, None),
            KeyBinding::new("cmd-v", Paste, None),
            KeyBinding::new("cmd-k", ClearScreen, None),
            KeyBinding::new("cmd-,", OpenSettings, None),
            KeyBinding::new("ctrl-,", OpenSettings, None),
            KeyBinding::new("ctrl-shift-t", NewTab, None),
            KeyBinding::new("ctrl-shift-w", CloseTab, None),
            KeyBinding::new("ctrl-shift-c", Copy, None),
            KeyBinding::new("ctrl-shift-v", Paste, None),
        ]);
        cx.set_menus(vec![
            Menu {
                name: "Ghostterm".into(),
                items: vec![
                    MenuItem::action("Settings…", OpenSettings),
                    MenuItem::separator(),
                    MenuItem::action("Quit Ghostterm", Quit),
                ],
            },
            Menu {
                name: "File".into(),
                items: vec![
                    MenuItem::action("New Window", NewWindow),
                    MenuItem::action("New Tab", NewTab),
                    MenuItem::separator(),
                    MenuItem::action("Close", CloseTab),
                ],
            },
            Menu {
                name: "Edit".into(),
                items: vec![MenuItem::action("Copy", Copy), MenuItem::action("Paste", Paste)],
            },
            Menu {
                name: "View".into(),
                items: vec![MenuItem::action("Clear Screen", ClearScreen)],
            },
        ]);
        cx.set_dock_menu(vec![MenuItem::action("New Window", NewWindow)]);

        open_workspace_window(cx);
        cx.activate(true);
    });
}

fn open_workspace_window(cx: &mut App) {
    let _ = cx.open_window(workspace_window_options(cx), |window, cx| cx.new(|cx| Workspace::new(window, cx)));
}

fn workspace_window_options(cx: &App) -> WindowOptions {
    let window_size = size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT));
    let mut bounds = Bounds::centered(None, window_size, cx);
    let stagger = cx.windows().len() as f32;
    bounds.origin = bounds.origin + point(px(stagger * NEW_WINDOW_OFFSET), px(stagger * NEW_WINDOW_OFFSET));

    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some("Ghostterm".into()),
            appears_transparent: false,
            traffic_light_position: None,
        }),
        focus: true,
        show: true,
        kind: gpui::WindowKind::Normal,
        is_movable: true,
        display_id: None,
        window_min_size: Some(size(px(640.0), px(400.0))),
        window_background: gpui::WindowBackgroundAppearance::Opaque,
        app_id: Some(APP_ID.into()),
        is_resizable: true,
        is_minimizable: true,
        window_decorations: None,
        tabbing_identifier: Some("Ghostterm".into()),
    }
}
