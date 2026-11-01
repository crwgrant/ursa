use gpui::{Menu, MenuItem};

use crate::{
    ActivateSession, ActivateTab, ClearScreen, CloseSession, CloseTab, Copy, FocusNextPane, FocusPrevPane, NewSession, NewTab,
    NewWindow, OpenAbout, OpenSettings, Paste, Quit, SplitDown, SplitRight,
};

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
            disabled: false,
        },
        native_file_menu(sessions_enabled),
        Menu {
            name: "Edit".into(),
            items: vec![MenuItem::action("Copy", Copy), MenuItem::action("Paste", Paste)],
            disabled: false,
        },
        native_view_menu(sessions_enabled),
        Menu {
            name: "Window".into(),
            items: vec![MenuItem::action("Close", CloseTab)],
            disabled: false,
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
        disabled: false,
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
        disabled: false,
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

#[cfg(test)]
mod tests {
    use gpui::MenuItem;

    use super::native_menus;

    fn labels(sessions_enabled: bool) -> Vec<(String, Vec<String>)> {
        native_menus(sessions_enabled)
            .into_iter()
            .map(|menu| {
                let items = menu
                    .items
                    .into_iter()
                    .filter_map(|item| match item {
                        MenuItem::Action { name, .. } => Some(name.to_string()),
                        _ => None,
                    })
                    .collect();
                (menu.name.to_string(), items)
            })
            .collect()
    }

    #[test]
    fn window_menus_include_quit_and_help() {
        let menus = labels(true);
        assert_eq!(menus[0].0, "Ursa");
        assert!(menus[0].1.contains(&"About Ursa".to_string()));
        assert!(menus[0].1.contains(&"Settings…".to_string()));
        assert!(menus[0].1.contains(&"Quit Ursa".to_string()));
        assert_eq!(menus[1].0, "File");
        assert!(!menus[1].1.contains(&"Settings…".to_string()));
    }

    #[test]
    fn window_menus_hide_session_actions_when_sidebar_off() {
        let with_sessions = labels(true);
        let without_sessions = labels(false);
        assert!(with_sessions[1].1.contains(&"New Session".to_string()));
        assert!(!without_sessions[1].1.contains(&"New Session".to_string()));
        assert!(with_sessions[3].1.contains(&"Session 1".to_string()));
        assert!(!without_sessions[3].1.contains(&"Session 1".to_string()));
    }
}
