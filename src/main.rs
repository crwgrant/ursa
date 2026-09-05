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
    AnyElement, AnyView, App, Application, Bounds, Context, CursorStyle, DragMoveEvent, KeyBinding, Menu, MenuItem, MouseButton,
    MouseDownEvent, SharedString, TitlebarOptions, Window, WindowBounds, WindowOptions, actions, div, point, prelude::*, px, rgb,
    size,
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
    sidebar_width: f32,
    sidebar_split_locked: bool,
    dragging_tab: Option<usize>,
    tab_insert_at: Option<usize>,
}

#[derive(Clone)]
struct TabDrag {
    index: usize,
    title: SharedString,
}

struct TabDragPreview {
    title: SharedString,
    background: u32,
    text: u32,
    accent: u32,
}

impl Render for TabDragPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let background = self.background;
        let text = self.text;
        let accent = self.accent;
        div()
            .px_3()
            .py_2()
            .rounded_md()
            .bg(rgb(background))
            .shadow_md()
            .flex()
            .items_center()
            .gap_2()
            .child(div().w(px(6.0)).h(px(6.0)).rounded_full().bg(rgb(accent)))
            .child(div().text_sm().text_color(rgb(text)).child(self.title.clone()))
    }
}

#[derive(Clone)]
struct SidebarSplit;

struct SplitDragPreview;

impl Render for SplitDragPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

impl Workspace {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let first = cx.new(|cx| Session::spawn(0, window, cx));
        let workspace = Self {
            tabs: vec![first],
            active: 0,
            sidebar_width: config::restored_sidebar_width(),
            sidebar_split_locked: false,
            dragging_tab: None,
            tab_insert_at: None,
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
        cx.observe_window_activation(window, |_, _, cx| cx.notify()).detach();
        cx.observe_window_bounds(window, |_, window, cx| {
            config::save_window_state(window, cx);
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

    fn can_copy(&self, cx: &App) -> bool {
        self.tabs.get(self.active).is_some_and(|tab| tab.read(cx).has_selection())
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

    fn set_sidebar_width(&mut self, width: f32, window: &Window, persist: bool, cx: &mut Context<Self>) {
        let width = config::clamp_sidebar_width(width, f32::from(window.viewport_size().width));
        if (self.sidebar_width - width).abs() < 0.5 {
            if persist {
                config::save_sidebar_width(self.sidebar_width);
            }
            return;
        }
        self.sidebar_width = width;
        if persist {
            config::save_sidebar_width(width);
        }
        cx.notify();
    }

    fn reset_sidebar_width(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.sidebar_split_locked = true;
        self.set_sidebar_width(theme::SIDEBAR_WIDTH, window, true, cx);
    }

    fn finish_sidebar_resize(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.sidebar_split_locked = false;
        self.set_sidebar_width(self.sidebar_width, window, true, cx);
    }

    fn set_tab_insert_at(&mut self, from: usize, insert_at: usize, cx: &mut Context<Self>) {
        self.dragging_tab = Some(from);
        if self.tab_insert_at != Some(insert_at) {
            self.tab_insert_at = Some(insert_at);
            cx.notify();
        }
    }

    fn drop_tab(&mut self, from: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(insert_at) = self.tab_insert_at {
            self.reorder_tab(from, insert_at, window, cx);
        }
        self.dragging_tab = None;
        self.tab_insert_at = None;
        cx.notify();
    }

    fn reorder_tab(&mut self, from: usize, insert_at: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dest) = tab_destination(from, insert_at, self.tabs.len()) else {
            return;
        };
        let tab = self.tabs.remove(from);
        self.tabs.insert(dest, tab);
        self.active = active_after_reorder(self.active, from, dest);
        self.focus_active(window, cx);
        cx.notify();
    }
}

fn tab_destination(from: usize, insert_at: usize, len: usize) -> Option<usize> {
    if from >= len || insert_at > len {
        return None;
    }
    if insert_at == from || insert_at == from + 1 {
        return None;
    }
    Some(if insert_at > from { insert_at - 1 } else { insert_at })
}

fn active_after_reorder(active: usize, from: usize, dest: usize) -> usize {
    if active == from {
        dest
    } else if from < active && dest >= active {
        active - 1
    } else if from > active && dest <= active {
        active + 1
    } else {
        active
    }
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !cx.has_active_drag() {
            self.dragging_tab = None;
            self.tab_insert_at = None;
        }
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
            .when(self.can_copy(cx), |el| {
                el.on_action(cx.listener(|this, _: &Copy, _window, cx| this.copy_active(cx)))
            })
            .when(session::clipboard_has_text(cx), |el| {
                el.on_action(cx.listener(|this, _: &Paste, _window, cx| this.paste_active(cx)))
            })
            .on_action(cx.listener(|this, _: &ClearScreen, _window, cx| this.clear_active(cx)))
            .on_action(|_: &OpenSettings, _window, cx| crate::settings::open(cx))
            .child(self.render_sidebar(&tabs, cx))
            .child(self.render_split(cx))
            .child(self.render_terminal())
            .child(notify::overlay(cx))
    }
}

impl Workspace {
    fn render_sidebar(&self, tabs: &[(usize, SharedString, bool)], cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        div()
            .w(px(self.sidebar_width))
            .h_full()
            .bg(rgb(colors.sidebar))
            .flex()
            .flex_col()
            .child(
                div()
                    .id("sessions-header")
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .py_3()
                    .when(tabs.len() > 1, |header| {
                        header
                            .on_drag_move(cx.listener(|this, event: &DragMoveEvent<TabDrag>, _, cx| {
                                if tab_drag_is_over(event) {
                                    this.set_tab_insert_at(event.drag(cx).index, 0, cx);
                                }
                            }))
                            .on_drop(cx.listener(|this, drag: &TabDrag, window, cx| {
                                this.drop_tab(drag.index, window, cx);
                            }))
                    })
                    .child(
                        div()
                            .text_xs()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgb(colors.text_dim))
                            .child("SESSIONS"),
                    )
                    .child(new_tab_button(cx)),
            )
            .child(self.render_tab_list(tabs, cx))
            .child(settings_button(cx))
    }

    fn render_tab_list(&self, tabs: &[(usize, SharedString, bool)], cx: &mut Context<Self>) -> impl IntoElement {
        let can_reorder = tabs.len() > 1;
        let insert_at = self.tab_insert_at;
        let dragging_tab = self.dragging_tab;
        let last = tabs.len().saturating_sub(1);
        let mut items: Vec<AnyElement> = Vec::new();
        items.push(tab_list_edge("tab-list-start", 0, can_reorder, cx).into_any_element());
        for (index, title, selected) in tabs {
            let insert = if insert_at == Some(*index) {
                Some(TabInsert::Before)
            } else if insert_at == Some(*index + 1) && *index == last {
                Some(TabInsert::After)
            } else {
                None
            };
            items.push(
                tab_row(*index, title.clone(), *selected, dragging_tab == Some(*index), insert, can_reorder, cx)
                    .into_any_element(),
            );
        }
        items.push(tab_list_edge("tab-list-end", tabs.len(), can_reorder, cx).into_any_element());
        div()
            .id("tab-list")
            .flex()
            .flex_col()
            .flex_1()
            .px_2()
            .gap_1()
            .overflow_y_scroll()
            .children(items)
            .when(can_reorder, |list| {
                list.on_drop(cx.listener(|this, drag: &TabDrag, window, cx| {
                    this.drop_tab(drag.index, window, cx);
                }))
            })
    }

    fn render_terminal(&self) -> impl IntoElement {
        div().flex_1().h_full().min_w_0().child(self.tabs[self.active].clone())
    }

    fn render_split(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        div()
            .relative()
            .w(px(1.0))
            .h_full()
            .flex_shrink_0()
            .bg(rgb(colors.sidebar_border))
            .child(
                div()
                    .id("sidebar-split")
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(-3.0))
                    .w(px(7.0))
                    .cursor(CursorStyle::ResizeColumn)
                    .hover(move |style| style.bg(rgb(colors.accent)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            if event.click_count >= 2 {
                                this.reset_sidebar_width(window, cx);
                            } else {
                                this.sidebar_split_locked = false;
                            }
                        }),
                    )
                    .on_drag(SidebarSplit, |_, _, _, cx| cx.new(|_| SplitDragPreview))
                    .on_drag_move(cx.listener(|this, event: &DragMoveEvent<SidebarSplit>, window, cx| {
                        if this.sidebar_split_locked {
                            return;
                        }
                        this.set_sidebar_width(f32::from(event.event.position.x), window, false, cx);
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            this.finish_sidebar_resize(window, cx);
                        }),
                    )
                    .on_mouse_up_out(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            this.finish_sidebar_resize(window, cx);
                        }),
                    ),
            )
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum TabInsert {
    Before,
    After,
}

fn tab_drag_is_over(event: &DragMoveEvent<TabDrag>) -> bool {
    event.bounds.contains(&event.event.position)
}

fn tab_insert_line(cx: &App) -> impl IntoElement {
    let colors = theme::colors(cx);
    div().w_full().h(px(2.0)).rounded_full().bg(rgb(colors.accent))
}

fn tab_list_edge(id: &'static str, insert_at: usize, can_reorder: bool, cx: &mut Context<Workspace>) -> impl IntoElement {
    div()
        .id(id)
        .when(id == "tab-list-end", |edge| edge.flex_1().min_h(px(12.0)))
        .when(id == "tab-list-start", |edge| edge.h(px(8.0)))
        .when(can_reorder, |edge| {
            edge.on_drag_move(cx.listener(move |this, event: &DragMoveEvent<TabDrag>, _, cx| {
                if tab_drag_is_over(event) {
                    this.set_tab_insert_at(event.drag(cx).index, insert_at, cx);
                }
            }))
            .on_drop(cx.listener(|this, drag: &TabDrag, window, cx| {
                this.drop_tab(drag.index, window, cx);
            }))
        })
}

fn tab_row(
    index: usize,
    title: SharedString,
    selected: bool,
    dragging: bool,
    insert: Option<TabInsert>,
    can_reorder: bool,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let colors = theme::colors(cx);
    let background = if selected {
        rgb(colors.tab_active)
    } else {
        rgb(colors.sidebar)
    };
    let drag = TabDrag {
        index,
        title: title.clone(),
    };
    let on_select = cx.listener(move |this, _event, window, cx| this.select_tab(index, window, cx));
    let on_drag_move = cx.listener(move |this, event: &DragMoveEvent<TabDrag>, _, cx| {
        if !tab_drag_is_over(event) {
            return;
        }
        let y = event.event.position.y - event.bounds.origin.y;
        let insert_at = if y < event.bounds.size.height * 0.5 {
            index
        } else {
            index + 1
        };
        this.set_tab_insert_at(event.drag(cx).index, insert_at, cx);
    });
    let on_drop = cx.listener(move |this, drag: &TabDrag, window, cx| {
        this.drop_tab(drag.index, window, cx);
    });
    let close = close_tab_button(index, cx);

    div()
        .id(("tab", index))
        .relative()
        .w_full()
        .rounded_md()
        .px_3()
        .py_2()
        .bg(background)
        .when(dragging, |row| row.opacity(0.4))
        .when(can_reorder, |row| row.cursor_move())
        .when(!can_reorder, |row| row.cursor_pointer())
        .hover(move |style| if selected { style } else { style.bg(rgb(colors.tab_hover)) })
        .on_mouse_down(MouseButton::Left, on_select)
        .when(can_reorder, |row| {
            row.on_drag(drag, |drag, _, _, cx| {
                let colors = theme::colors(cx);
                cx.new(|_| TabDragPreview {
                    title: drag.title.clone(),
                    background: colors.tab_active,
                    text: colors.text,
                    accent: colors.accent,
                })
            })
            .on_drag_move(on_drag_move)
            .on_drop(on_drop)
        })
        .when_some(insert, |row, edge| {
            row.child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .when(edge == TabInsert::Before, |line| line.top_0())
                    .when(edge == TabInsert::After, |line| line.bottom_0())
                    .child(tab_insert_line(cx)),
            )
        })
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
                .child(close),
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
        cx.on_action(|_: &Quit, cx| {
            save_workspace_windows(cx);
            cx.quit();
        });
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
            Menu {
                name: "Window".into(),
                items: vec![MenuItem::action("Close", CloseTab)],
            },
        ]);
        cx.set_dock_menu(vec![MenuItem::action("New Window", NewWindow)]);

        open_workspace_window(cx);
        cx.activate(true);
    });
}

fn open_workspace_window(cx: &mut App) {
    let _ = cx.open_window(workspace_window_options(cx), |window, cx| {
        window.on_window_should_close(cx, |window, cx| {
            config::save_window_state(window, cx);
            true
        });
        cx.new(|cx| Workspace::new(window, cx))
    });
}

pub(crate) fn reset_workspace_sidebars(cx: &mut App) {
    for handle in cx.windows() {
        let Some(workspace) = handle.downcast::<Workspace>() else {
            continue;
        };
        let _ = workspace.update(cx, |this, window, cx| {
            this.reset_sidebar_width(window, cx);
        });
    }
}

fn save_workspace_windows(cx: &mut App) {
    for handle in cx.windows() {
        let Some(workspace) = handle.downcast::<Workspace>() else {
            continue;
        };
        let _ = workspace.update(cx, |_, window, cx| {
            config::save_window_state(window, cx);
        });
    }
}

fn workspace_window_options(cx: &App) -> WindowOptions {
    let window_size = size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT));
    let (mut window_bounds, display_id) =
        config::restored_window(cx).unwrap_or_else(|| (WindowBounds::Windowed(Bounds::centered(None, window_size, cx)), None));
    let stagger = cx.windows().len() as f32;
    if stagger > 0.0 {
        let offset = point(px(stagger * NEW_WINDOW_OFFSET), px(stagger * NEW_WINDOW_OFFSET));
        match &mut window_bounds {
            WindowBounds::Windowed(bounds) | WindowBounds::Maximized(bounds) | WindowBounds::Fullscreen(bounds) => {
                bounds.origin = bounds.origin + offset;
            }
        }
    }

    WindowOptions {
        window_bounds: Some(window_bounds),
        titlebar: Some(TitlebarOptions {
            title: Some("Ghostterm".into()),
            appears_transparent: false,
            traffic_light_position: None,
        }),
        focus: true,
        show: true,
        kind: gpui::WindowKind::Normal,
        is_movable: true,
        display_id,
        window_min_size: Some(size(px(config::WINDOW_MIN_WIDTH), px(config::WINDOW_MIN_HEIGHT))),
        window_background: gpui::WindowBackgroundAppearance::Opaque,
        app_id: Some(APP_ID.into()),
        is_resizable: true,
        is_minimizable: true,
        window_decorations: None,
        tabbing_identifier: Some("Ghostterm".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::{active_after_reorder, tab_destination};

    #[test]
    fn tab_destination_skips_same_slot() {
        assert_eq!(tab_destination(1, 1, 4), None);
        assert_eq!(tab_destination(1, 2, 4), None);
        assert_eq!(tab_destination(0, 0, 1), None);
        assert_eq!(tab_destination(3, 5, 4), None);
    }

    #[test]
    fn tab_destination_moves_around_neighbors() {
        assert_eq!(tab_destination(1, 0, 4), Some(0));
        assert_eq!(tab_destination(1, 3, 4), Some(2));
        assert_eq!(tab_destination(1, 4, 4), Some(3));
        assert_eq!(tab_destination(0, 4, 4), Some(3));
        assert_eq!(tab_destination(3, 0, 4), Some(0));
    }

    #[test]
    fn active_index_follows_moved_tab() {
        assert_eq!(active_after_reorder(1, 1, 3), 3);
        assert_eq!(active_after_reorder(2, 0, 2), 1);
        assert_eq!(active_after_reorder(0, 2, 0), 1);
        assert_eq!(active_after_reorder(1, 0, 2), 0);
        assert_eq!(active_after_reorder(1, 3, 0), 2);
        assert_eq!(active_after_reorder(1, 0, 3), 0);
    }
}
