mod frame;
mod input;
mod pty;
mod session;
mod theme;

use gpui::{
    actions, div, prelude::*, px, rgb, size, App, Application, Bounds, Context, KeyBinding,
    MouseButton, SharedString, TitlebarOptions, Window, WindowBounds, WindowOptions,
};
use session::Session;

actions!(ghostterm, [Quit, NewTab]);

struct Workspace {
    tabs: Vec<gpui::Entity<Session>>,
    active: usize,
}

impl Workspace {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let first = cx.new(|cx| Session::spawn(0, window, cx));
        Self {
            tabs: vec![first],
            active: 0,
        }
    }

    fn add_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let index = self.tabs.len();
        let tab = cx.new(|cx| Session::spawn(index, window, cx));
        self.tabs.push(tab);
        self.active = index;
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

    fn focus_active(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get(self.active) {
            tab.update(cx, |session, _cx| session.focus(window));
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
        .child("+")
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _event, window, cx| this.add_tab(window, cx)),
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
                .child(div().w(px(6.0)).h(px(6.0)).rounded_full().bg(if selected {
                    rgb(theme::ACCENT)
                } else {
                    rgb(theme::TEXT_DIM)
                }))
                .child(
                    div()
                        .text_sm()
                        .text_ellipsis()
                        .text_color(if selected {
                            rgb(theme::TEXT)
                        } else {
                            rgb(theme::TEXT_DIM)
                        })
                        .child(title),
                ),
        )
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1100.0), px(720.0)), cx);

        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-t", NewTab, None),
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
