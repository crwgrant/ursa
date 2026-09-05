use gpui::{
    Action, AnyElement, Context, Corner, InteractiveElement, IntoElement, Menu, MenuItem, MouseButton, MouseDownEvent,
    MouseMoveEvent, ParentElement, Pixels, Point, SharedString, StatefulInteractiveElement, Styled, Window, anchored, div, point,
    prelude::*, px, rgb,
};

use crate::{
    ActivateSession, ActivateTab, ClearScreen, CloseSession, CloseTab, Copy, FocusNextPane, FocusPrevPane, NewSession, NewTab,
    NewWindow, OpenAbout, OpenSettings, Paste, Quit, SplitDown, SplitRight, theme,
};

pub const MENU_BAR_HEIGHT: f32 = 28.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MenuId {
    File,
    Edit,
    View,
    Help,
}

pub struct OpenMenu {
    pub id: MenuId,
    pub origin: Point<Pixels>,
}

pub trait AppMenuState: Sized {
    fn toggle_app_menu(&mut self, id: MenuId, origin: Point<Pixels>, cx: &mut Context<Self>);
    fn hover_app_menu(&mut self, id: MenuId, origin: Point<Pixels>, cx: &mut Context<Self>);
    fn dismiss_app_menu(&mut self, cx: &mut Context<Self>) -> bool;
    fn run_app_menu_action(&mut self, action: Box<dyn Action>, window: &mut Window, cx: &mut Context<Self>);
}

struct BarMenu {
    id: MenuId,
    name: &'static str,
    items: Vec<BarItem>,
}

enum BarItem {
    Action {
        id: SharedString,
        label: SharedString,
        shortcut: Option<SharedString>,
        action: Box<dyn Action>,
    },
    Separator,
}

pub fn native_menus(sessions_enabled: bool) -> Vec<Menu> {
    vec![
        Menu {
            name: "Ursa".into(),
            items: vec![
                MenuItem::action("About Ursa", OpenAbout),
                MenuItem::separator(),
                MenuItem::action("Settings…", OpenSettings),
                MenuItem::separator(),
                MenuItem::action("Quit Ursa", Quit),
            ],
        },
        native_file_menu(sessions_enabled),
        Menu {
            name: "Edit".into(),
            items: vec![MenuItem::action("Copy", Copy), MenuItem::action("Paste", Paste)],
        },
        native_view_menu(sessions_enabled),
        Menu {
            name: "Window".into(),
            items: vec![MenuItem::action("Close", CloseTab)],
        },
    ]
}

fn native_file_menu(sessions_enabled: bool) -> Menu {
    let mut items = vec![MenuItem::action("New Window", NewWindow)];
    if sessions_enabled {
        items.push(MenuItem::action("New Session", NewSession));
    }
    items.push(MenuItem::action("New Tab", NewTab));
    items.push(MenuItem::action("Split Right", SplitRight));
    items.push(MenuItem::action("Split Down", SplitDown));
    items.push(MenuItem::separator());
    items.push(MenuItem::action("Close Tab", CloseTab));
    if sessions_enabled {
        items.push(MenuItem::action("Close Session", CloseSession));
    }
    Menu {
        name: "File".into(),
        items,
    }
}

fn native_view_menu(sessions_enabled: bool) -> Menu {
    let mut items = vec![
        MenuItem::action("Clear Screen", ClearScreen),
        MenuItem::separator(),
        MenuItem::action("Focus Next Pane", FocusNextPane),
        MenuItem::action("Focus Previous Pane", FocusPrevPane),
        MenuItem::separator(),
    ];
    if sessions_enabled {
        items.push(MenuItem::action("Close Session", CloseSession));
        items.push(MenuItem::separator());
    }
    for number in 1..=9 {
        items.push(MenuItem::action(format!("Tab {number}"), ActivateTab { index: number - 1 }));
    }
    if sessions_enabled {
        items.push(MenuItem::separator());
        for number in 1..=9 {
            items.push(MenuItem::action(format!("Session {number}"), ActivateSession { index: number - 1 }));
        }
    }
    Menu {
        name: "View".into(),
        items,
    }
}

fn window_menus(sessions_enabled: bool) -> Vec<BarMenu> {
    vec![
        BarMenu {
            id: MenuId::File,
            name: "File",
            items: file_items(sessions_enabled),
        },
        BarMenu {
            id: MenuId::Edit,
            name: "Edit",
            items: vec![
                action_item("edit-copy", "Copy", Some(copy_shortcut()), Copy),
                action_item("edit-paste", "Paste", Some(paste_shortcut()), Paste),
            ],
        },
        BarMenu {
            id: MenuId::View,
            name: "View",
            items: view_items(sessions_enabled),
        },
        BarMenu {
            id: MenuId::Help,
            name: "Help",
            items: vec![action_item("help-about", "About Ursa", None::<&str>, OpenAbout)],
        },
    ]
}

fn file_items(sessions_enabled: bool) -> Vec<BarItem> {
    let mut items = vec![action_item(
        "file-new-window",
        "New Window",
        Some(new_window_shortcut()),
        NewWindow,
    )];
    if sessions_enabled {
        items.push(action_item("file-new-session", "New Session", Some(new_session_shortcut()), NewSession));
    }
    items.push(action_item("file-new-tab", "New Tab", Some(new_tab_shortcut()), NewTab));
    items.push(action_item("file-split-right", "Split Right", Some(split_right_shortcut()), SplitRight));
    items.push(action_item("file-split-down", "Split Down", Some(split_down_shortcut()), SplitDown));
    items.push(BarItem::Separator);
    items.push(action_item("file-close-tab", "Close Tab", Some(close_tab_shortcut()), CloseTab));
    if sessions_enabled {
        items.push(action_item(
            "file-close-session",
            "Close Session",
            Some(close_session_shortcut()),
            CloseSession,
        ));
    }
    items.push(BarItem::Separator);
    items.push(action_item("file-settings", "Settings…", Some(settings_shortcut()), OpenSettings));
    items.push(BarItem::Separator);
    items.push(action_item("file-quit", "Quit Ursa", None::<&str>, Quit));
    items
}

fn view_items(sessions_enabled: bool) -> Vec<BarItem> {
    let mut items = vec![
        action_item(
            "view-clear",
            "Clear Screen",
            cfg!(target_os = "macos").then_some(clear_screen_shortcut()),
            ClearScreen,
        ),
        BarItem::Separator,
        action_item("view-next-pane", "Focus Next Pane", Some(focus_next_shortcut()), FocusNextPane),
        action_item("view-prev-pane", "Focus Previous Pane", Some(focus_prev_shortcut()), FocusPrevPane),
        BarItem::Separator,
    ];
    for number in 1..=9 {
        items.push(action_item(
            format!("view-tab-{number}"),
            format!("Tab {number}"),
            Some(tab_shortcut(number)),
            ActivateTab { index: number - 1 },
        ));
    }
    if sessions_enabled {
        items.push(BarItem::Separator);
        for number in 1..=9 {
            items.push(action_item(
                format!("view-session-{number}"),
                format!("Session {number}"),
                Some(session_shortcut(number)),
                ActivateSession { index: number - 1 },
            ));
        }
    }
    items
}

fn action_item(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    shortcut: Option<impl Into<SharedString>>,
    action: impl Action,
) -> BarItem {
    BarItem::Action {
        id: id.into(),
        label: label.into(),
        shortcut: shortcut.map(Into::into),
        action: Box::new(action),
    }
}

pub fn new_session_shortcut() -> &'static str {
    if cfg!(target_os = "macos") { "⌘⇧T" } else { "Ctrl+Alt+T" }
}

pub fn new_tab_shortcut() -> &'static str {
    if cfg!(target_os = "macos") { "⌘T" } else { "Ctrl+Shift+T" }
}

pub fn close_tab_shortcut() -> &'static str {
    if cfg!(target_os = "macos") { "⌘W" } else { "Ctrl+Shift+W" }
}

pub fn close_session_shortcut() -> &'static str {
    if cfg!(target_os = "macos") { "⌘⇧W" } else { "Ctrl+Alt+W" }
}

pub fn settings_shortcut() -> &'static str {
    if cfg!(target_os = "macos") { "⌘," } else { "Ctrl+," }
}

fn new_window_shortcut() -> &'static str {
    if cfg!(target_os = "macos") { "⌘N" } else { "Ctrl+N" }
}

fn copy_shortcut() -> &'static str {
    if cfg!(target_os = "macos") { "⌘C" } else { "Ctrl+Shift+C" }
}

fn paste_shortcut() -> &'static str {
    if cfg!(target_os = "macos") { "⌘V" } else { "Ctrl+Shift+V" }
}

fn split_right_shortcut() -> &'static str {
    if cfg!(target_os = "macos") { "⌘D" } else { "Ctrl+Shift+D" }
}

fn split_down_shortcut() -> &'static str {
    if cfg!(target_os = "macos") { "⌘⇧D" } else { "Ctrl+Alt+D" }
}

fn clear_screen_shortcut() -> &'static str {
    "⌘K"
}

fn focus_next_shortcut() -> &'static str {
    if cfg!(target_os = "macos") { "⌘]" } else { "Ctrl+Shift+]" }
}

fn focus_prev_shortcut() -> &'static str {
    if cfg!(target_os = "macos") { "⌘[" } else { "Ctrl+Shift+[" }
}

fn tab_shortcut(number: usize) -> String {
    if cfg!(target_os = "macos") {
        format!("⌘{number}")
    } else {
        format!("Ctrl+Shift+{number}")
    }
}

fn session_shortcut(number: usize) -> String {
    if cfg!(target_os = "macos") {
        format!("⌃{number}")
    } else {
        format!("Ctrl+{number}")
    }
}

fn menu_origin(event_x: Pixels) -> Point<Pixels> {
    point(event_x, px(MENU_BAR_HEIGHT))
}

pub fn render_bar<S: AppMenuState + 'static>(
    open: Option<MenuId>,
    sessions_enabled: bool,
    cx: &mut Context<S>,
) -> impl IntoElement {
    let colors = theme::colors(cx);
    let menus = window_menus(sessions_enabled);
    div()
        .id("app-menu-bar")
        .flex()
        .flex_shrink_0()
        .items_center()
        .h(px(MENU_BAR_HEIGHT))
        .px_1()
        .bg(rgb(colors.sidebar))
        .border_b_1()
        .border_color(rgb(colors.sidebar_border))
        .text_sm()
        .children(menus.into_iter().map(|menu| render_title(menu, open, cx)))
}

fn render_title<S: AppMenuState + 'static>(menu: BarMenu, open: Option<MenuId>, cx: &mut Context<S>) -> impl IntoElement {
    let colors = theme::colors(cx);
    let id = menu.id;
    let selected = open == Some(id);
    div()
        .id(("app-menu", id as u32))
        .h_full()
        .px_2()
        .flex()
        .items_center()
        .rounded_sm()
        .cursor_pointer()
        .when(selected, |el| el.bg(rgb(colors.tab_hover)))
        .hover(move |style| style.bg(rgb(colors.tab_hover)))
        .child(menu.name)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                this.toggle_app_menu(id, menu_origin(event.position.x), cx);
                cx.stop_propagation();
            }),
        )
        .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _window, cx| {
            this.hover_app_menu(id, menu_origin(event.position.x), cx);
        }))
}

pub fn render_popovers<S: AppMenuState + 'static>(
    open: Option<&OpenMenu>,
    sessions_enabled: bool,
    cx: &mut Context<S>,
) -> Vec<AnyElement> {
    let Some(open) = open else {
        return Vec::new();
    };
    let Some(menu) = window_menus(sessions_enabled).into_iter().find(|menu| menu.id == open.id) else {
        return Vec::new();
    };
    let colors = theme::colors(cx);
    vec![
        div()
            .id("app-menu-layer")
            .absolute()
            .top(px(MENU_BAR_HEIGHT))
            .left_0()
            .right_0()
            .bottom_0()
            .child(div().id("app-menu-dismiss").absolute().inset_0().occlude().on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _event, _window, cx| {
                    this.dismiss_app_menu(cx);
                    cx.stop_propagation();
                }),
            ))
            .child(
                anchored().position(open.origin).anchor(Corner::TopLeft).child(
                    div()
                        .id(("app-menu-dropdown", menu.id as u32))
                        .occlude()
                        .min_w(px(220.0))
                        .max_w(px(280.0))
                        .max_h(px(360.0))
                        .overflow_y_scroll()
                        .py_1()
                        .rounded_md()
                        .bg(rgb(colors.tooltip))
                        .border_1()
                        .border_color(rgb(colors.sidebar_border))
                        .shadow_md()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .children(menu.items.into_iter().map(|item| render_item(item, cx))),
                ),
            )
            .into_any_element(),
    ]
}

fn render_item<S: AppMenuState + 'static>(item: BarItem, cx: &mut Context<S>) -> AnyElement {
    let colors = theme::colors(cx);
    match item {
        BarItem::Separator => div()
            .h(px(1.0))
            .mx_1()
            .my_1()
            .bg(rgb(colors.sidebar_border))
            .into_any_element(),
        BarItem::Action {
            id,
            label,
            shortcut,
            action,
        } => div()
            .id(id)
            .w_full()
            .px_3()
            .py_1()
            .flex()
            .items_center()
            .justify_between()
            .gap_4()
            .text_sm()
            .text_color(rgb(colors.text))
            .cursor_pointer()
            .hover(move |style| style.bg(rgb(colors.tab_hover)))
            .child(label)
            .children(shortcut.map(|shortcut| div().text_xs().text_color(rgb(colors.text_dim)).child(shortcut)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _event, window, cx| {
                    cx.stop_propagation();
                    this.run_app_menu_action(action.boxed_clone(), window, cx);
                }),
            )
            .into_any_element(),
    }
}

#[cfg(test)]
mod tests {
    use super::window_menus;

    fn labels(sessions_enabled: bool) -> Vec<(String, Vec<String>)> {
        window_menus(sessions_enabled)
            .into_iter()
            .map(|menu| {
                let items = menu
                    .items
                    .into_iter()
                    .filter_map(|item| match item {
                        super::BarItem::Action { label, .. } => Some(label.to_string()),
                        super::BarItem::Separator => None,
                    })
                    .collect();
                (menu.name.to_string(), items)
            })
            .collect()
    }

    #[test]
    fn window_menus_include_quit_and_help() {
        let menus = labels(true);
        assert_eq!(menus[0].0, "File");
        assert!(menus[0].1.contains(&"Settings…".to_string()));
        assert!(menus[0].1.contains(&"Quit Ursa".to_string()));
        assert_eq!(menus[3].0, "Help");
        assert!(!menus[3].1.contains(&"Settings…".to_string()));
        assert!(menus[3].1.contains(&"About Ursa".to_string()));
    }

    #[test]
    fn window_menus_hide_session_actions_when_sidebar_off() {
        let with_sessions = labels(true);
        let without_sessions = labels(false);
        assert!(with_sessions[0].1.contains(&"New Session".to_string()));
        assert!(!without_sessions[0].1.contains(&"New Session".to_string()));
        assert!(with_sessions[2].1.contains(&"Session 1".to_string()));
        assert!(!without_sessions[2].1.contains(&"Session 1".to_string()));
    }
}
