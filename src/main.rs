mod frame;
mod input;
mod pty;
mod session;
mod theme;

use gpui::{
    actions, div, prelude::*, px, rgb, size, AnyView, App, Application, Bounds, Context,
    KeyBinding, MouseButton, SharedString, TitlebarOptions, Window, WindowBounds, WindowOptions,
};
use session::{Session, SessionEvent};

actions!(ghostterm, [Quit, NewTab, CloseTab, Copy, Paste]);

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
        workspace
    }

    fn subscribe_session(&self, index: usize, window: &Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(index).cloned() else {
            return;
        };
        cx.subscribe_in(
            &tab,
            window,
            |this, session, event: &SessionEvent, window, cx| {
                if matches!(event, SessionEvent::Exited) {
                    if let Some(index) = this.tabs.iter().position(|tab| tab == session) {
                        this.close_tab(index, window, cx);
                    }
                }
            },
        )
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

        div()
            .flex()
            .size_full()
            .bg(rgb(theme::WINDOW))
            .text_color(rgb(theme::TEXT))
            .font_family("Menlo")
            .on_action(cx.listener(|this, _: &NewTab, window, cx| this.add_tab(window, cx)))
            .on_action(
                cx.listener(|this, _: &CloseTab, window, cx| this.close_active_tab(window, cx)),
            )
            .on_action(cx.listener(|this, _: &Copy, _window, cx| this.copy_active(cx)))
            .on_action(cx.listener(|this, _: &Paste, _window, cx| this.paste_active(cx)))
            .child(self.render_sidebar(&tabs, cx))
            .child(self.render_terminal())
    }
}

impl Workspace {
    fn render_sidebar(
        &self,
        tabs: &[(usize, SharedString, bool)],
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .w(px(theme::SIDEBAR_WIDTH))
            .h_full()
            .bg(rgb(theme::SIDEBAR))
            .border_r_1()
            .border_color(rgb(theme::SIDEBAR_BORDER))
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
                            .text_color(rgb(theme::TEXT_DIM))
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
                    .children(tabs.iter().map(|(index, title, selected)| {
                        tab_row(*index, title.clone(), *selected, cx)
                    })),
            )
    }

    fn render_terminal(&self) -> impl IntoElement {
        div()
            .flex_1()
            .h_full()
            .min_w_0()
            .child(self.tabs[self.active].clone())
    }
}

fn new_tab_button(cx: &mut Context<Workspace>) -> impl IntoElement {
    div()
        .id("new-tab")
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
        .tooltip(action_tooltip("New Tab", "⌘T"))
        .child("+")
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _event, window, cx| this.add_tab(window, cx)),
        )
}

fn close_tab_button(index: usize, cx: &mut Context<Workspace>) -> impl IntoElement {
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
        .text_color(rgb(theme::TEXT_DIM))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(theme::BUTTON)).text_color(rgb(theme::TEXT)))
        .tooltip(action_tooltip("Close Tab", "⌘W"))
        .child("×")
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _event, window, cx| {
                cx.stop_propagation();
                this.close_tab(index, window, cx);
            }),
        )
}

fn tab_row(
    index: usize,
    title: SharedString,
    selected: bool,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let background = if selected {
        rgb(theme::TAB_ACTIVE)
    } else {
        rgb(theme::SIDEBAR)
    };

    div()
        .id(("tab", index))
        .w_full()
        .rounded_md()
        .px_3()
        .py_2()
        .bg(background)
        .cursor_pointer()
        .hover(|style| {
            if selected {
                style
            } else {
                style.bg(rgb(theme::TAB_HOVER))
            }
        })
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
                    rgb(theme::ACCENT)
                } else {
                    rgb(theme::TEXT_DIM)
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_sm()
                        .text_ellipsis()
                        .text_color(if selected {
                            rgb(theme::TEXT)
                        } else {
                            rgb(theme::TEXT_DIM)
                        })
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
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(rgb(theme::TOOLTIP))
            .border_1()
            .border_color(rgb(theme::SIDEBAR_BORDER))
            .shadow_md()
            .text_xs()
            .child(div().text_color(rgb(theme::TEXT)).child(self.label.clone()))
            .child(
                div()
                    .text_color(rgb(theme::TEXT_DIM))
                    .child(self.shortcut.clone()),
            )
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1100.0), px(720.0)), cx);

        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-t", NewTab, None),
            KeyBinding::new("cmd-w", CloseTab, None),
            KeyBinding::new("cmd-c", Copy, None),
            KeyBinding::new("cmd-v", Paste, None),
        ]);

        cx.open_window(
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
                app_id: None,
                is_resizable: true,
                is_minimizable: true,
                window_decorations: None,
                tabbing_identifier: None,
            },
            |window, cx| cx.new(|cx| Workspace::new(window, cx)),
        )
        .unwrap();

        cx.activate(true);
    });
}
