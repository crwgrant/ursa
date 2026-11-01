use gpui::{
    App, Bounds, Context, DisplayId, FocusHandle, Global, IntoElement, KeyDownEvent, Render, SharedString, TitlebarOptions,
    Window, WindowBounds, WindowOptions, prelude::*, px, size,
};
use gpui::component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    setting::{NumberFieldOptions, SettingField, SettingGroup, SettingItem, SettingPage, Settings},
    v_flex,
};

use crate::{config, notify, theme};

#[derive(Default)]
struct SettingsUi {
    window: Option<gpui::AnyWindowHandle>,
}

impl Global for SettingsUi {}

pub struct SettingsPage {
    focus: FocusHandle,
}

impl SettingsPage {
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
    if let Some(handle) = cx.try_global::<SettingsUi>().and_then(|ui| ui.window) {
        if crate::activate_window(handle, cx) {
            return;
        }
    }

    let display_id = crate::host_display_id(cx);
    match gpui::open_window(window_options(cx, display_id), cx, |window, cx| cx.new(|cx| SettingsPage::new(window, cx))) {
        Ok((handle, _)) => {
            crate::on_aux_window_close(handle, cx, |cx| {
                if cx.has_global::<SettingsUi>() {
                    cx.global_mut::<SettingsUi>().window = None;
                }
            });
            cx.default_global::<SettingsUi>().window = Some(handle);
        }
        Err(error) => notify::show(cx, format!("Couldn't open settings: {error}")),
    }
}

fn window_options(cx: &App, display_id: Option<DisplayId>) -> WindowOptions {
    let bounds = Bounds::centered(display_id, size(px(720.0), px(640.0)), cx);
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
        window_min_size: Some(size(px(520.0), px(480.0))),
        window_background: gpui::WindowBackgroundAppearance::Opaque,
        app_id: Some(crate::APP_ID.into()),
        is_resizable: true,
        is_minimizable: true,
        window_decorations: None,
        tabbing_identifier: Some("Ursa Settings".into()),
        ..Default::default()
    }
}

impl Render for SettingsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("settings")
            .track_focus(&self.focus)
            .size_full()
            .on_key_down(cx.listener(|this, event, window, cx| this.handle_key(event, window, cx)))
            .on_action(cx.listener(|this, _: &crate::CloseTab, window, cx| this.close_window(window, cx)))
            .child(settings_view(cx))
    }
}

fn settings_view(cx: &App) -> impl IntoElement {
    Settings::new("ursa-settings")
        .sidebar_width(px(168.0))
        .page(appearance_page(cx))
        .page(terminal_page(cx))
        .page(config_page(cx))
}

fn appearance_page(cx: &App) -> SettingPage {
    SettingPage::new("Appearance")
        .description("Theme and terminal font.")
        .group(
            SettingGroup::new()
                .title("Theme")
                .item(
                    SettingItem::new(
                        "Color theme",
                        SettingField::scrollable_dropdown(
                            theme::choices(cx)
                                .into_iter()
                                .map(|(id, label)| (SharedString::from(id), SharedString::from(label)))
                                .collect(),
                            |cx| SharedString::from(config::current(cx).theme),
                            |theme, cx| {
                                config::update(cx, |config| {
                                    config.theme = theme.to_string();
                                });
                            },
                        )
                        .default_value(SharedString::from(theme::DEFAULT_THEME)),
                    )
                    .description("Applies immediately to windows and the terminal."),
                )
                .item(SettingItem::render(|_, _, cx| {
                    theme::catalog(cx)
                        .error
                        .map(|message| format!("Could not load themes file: {message}"))
                        .unwrap_or_default()
                })),
        )
        .group(
            SettingGroup::new()
                .title("Font")
                .item(
                    SettingItem::new(
                        "Family",
                        SettingField::scrollable_dropdown(
                            font_family_options(cx),
                            |cx| config::font_family(cx),
                            |family, cx| {
                                config::update(cx, |config| {
                                    config.font_family = Some(family.to_string());
                                });
                            },
                        ),
                    )
                    .description("Installed fonts plus Ursa presets."),
                )
                .item(SettingItem::new(
                    "Size",
                    SettingField::dropdown(
                        font_size_options(cx),
                        |cx| SharedString::from(format_font_size(config::current(cx).font_size)),
                        |label, cx| {
                            if let Some(size) = parse_font_size(&label) {
                                config::update(cx, |config| {
                                    config.font_size = size;
                                });
                            }
                        },
                    )
                    .default_value(SharedString::from(format_font_size(theme::FONT_SIZE))),
                )),
        )
}

fn terminal_page(cx: &App) -> SettingPage {
    let sidebar_on = config::sessions_enabled(cx);
    SettingPage::new("Terminal")
        .description("Cursor, sessions, and scrollback.")
        .group(
            SettingGroup::new()
                .title("Behavior")
                .item(SettingItem::new(
                    "Cursor",
                    SettingField::dropdown(
                        enum_options(config::CursorShape::all(), config::CursorShape::as_str, config::CursorShape::label),
                        |cx| SharedString::from(config::current(cx).cursor_shape.as_str()),
                        |value, cx| {
                            if let Some(shape) = config::CursorShape::parse(&value) {
                                config::update(cx, |config| {
                                    config.cursor_shape = shape;
                                });
                            }
                        },
                    )
                    .default_value(SharedString::from(config::CursorShape::Bar.as_str())),
                ))
                .item(
                    SettingItem::new(
                        "When a session exits",
                        SettingField::dropdown(
                            enum_options(config::OnExit::all(), config::OnExit::as_str, config::OnExit::label),
                            |cx| SharedString::from(config::current(cx).on_exit.as_str()),
                            |value, cx| {
                                if let Some(on_exit) = config::OnExit::parse(&value) {
                                    config::update(cx, |config| {
                                        config.on_exit = on_exit;
                                    });
                                }
                            },
                        )
                        .default_value(SharedString::from(config::OnExit::Close.as_str())),
                    )
                    .description("Close the pane, or keep the exited session visible."),
                )
                .item(
                    SettingItem::new(
                        "Session sidebar",
                        SettingField::switch(
                            |cx| config::sessions_enabled(cx),
                            |on, cx| {
                                config::update(cx, |config| {
                                    config.session_sidebar = if on {
                                        config::SessionSidebar::On
                                    } else {
                                        config::SessionSidebar::Off
                                    };
                                });
                                crate::reset_workspace_sidebars(cx);
                            },
                        )
                        .default_value(true),
                    )
                    .description("Show multiple sessions in a sidebar."),
                )
                .item(
                    SettingItem::new(
                        "Windows shell",
                        SettingField::dropdown(
                            enum_options(
                                config::WindowsShell::choices(),
                                config::WindowsShell::as_str,
                                config::WindowsShell::label,
                            ),
                            |cx| SharedString::from(config::current(cx).windows_shell.resolved().as_str()),
                            |value, cx| {
                                if let Some(shell) = config::WindowsShell::parse(&value) {
                                    config::update(cx, |config| {
                                        config.windows_shell = shell;
                                    });
                                }
                            },
                        )
                        .default_value(SharedString::from(config::WindowsShell::Auto.as_str())),
                    )
                    .description("Used for new sessions when the sidebar is off.")
                    .disabled(!cfg!(windows) || sidebar_on),
                ),
        )
        .group(SettingGroup::new().title("Scrollback").item(
            SettingItem::new(
                "Lines",
                SettingField::number_input(
                    NumberFieldOptions {
                        min: f64::from(config::SCROLLBACK_MIN),
                        max: f64::from(config::SCROLLBACK_MAX),
                        step: 100.0,
                    },
                    |cx| f64::from(config::scrollback_lines(cx)),
                    |lines, cx| {
                        config::update(cx, |config| {
                            config.scrollback_lines = (lines.round() as u32).clamp(config::SCROLLBACK_MIN, config::SCROLLBACK_MAX);
                        });
                    },
                )
                .default_value(f64::from(config::DEFAULT_SCROLLBACK)),
            )
            .description(format!(
                "Lines kept above the viewport ({}–{}).",
                config::SCROLLBACK_MIN,
                config::SCROLLBACK_MAX
            )),
        ))
}

fn config_page(cx: &App) -> SettingPage {
    let path = config::display_path(cx);
    let load_error = config::load_error(cx);
    SettingPage::new("Config file")
        .description("Ursa writes these settings to a TOML file.")
        .group(
            SettingGroup::new()
                .title("File")
                .item(SettingItem::render(move |_, _, _| path.clone()))
                .item(SettingItem::render(move |_, _, _| {
                    load_error
                        .clone()
                        .map(|message| format!("Could not reload file: {message}"))
                        .unwrap_or_default()
                }))
                .item(SettingItem::render(|_, _, _| {
                    h_flex()
                        .gap_2()
                        .child(Button::new("open-config").secondary().label("Open File").on_click(|_, _, cx| {
                            if let Some(path) = config::ensure_file(cx) {
                                cx.open_with_system(&path);
                            }
                        }))
                        .child(
                            Button::new("reveal-config")
                                .secondary()
                                .label(reveal_label())
                                .on_click(|_, _, cx| {
                                    if let Some(path) = config::ensure_file(cx) {
                                        cx.reveal_path(&path);
                                    }
                                }),
                        )
                        .child(Button::new("reload-config").secondary().label("Reload").on_click(|_, _, cx| {
                            match config::reload(cx) {
                                Ok(()) => notify::show(cx, "Reloaded settings"),
                                Err(error) => notify::show(cx, format!("Config file error: {error}")),
                            }
                        }))
                }))
                .item(SettingItem::render(|_, _, _| {
                    Button::new("reset-config")
                        .danger()
                        .label("Reset to Defaults")
                        .on_click(|_, _, cx| {
                            config::reset(cx);
                            crate::reset_workspace_sidebars(cx);
                            notify::show(cx, "Settings reset");
                        })
                })),
        )
}

fn font_family_options(cx: &App) -> Vec<(SharedString, SharedString)> {
    let family = config::font_family(cx).to_string();
    let installed = cx.text_system().all_font_names();
    config::font_choices(&family, &installed)
        .into_iter()
        .map(|name| {
            let label = SharedString::from(name.clone());
            (SharedString::from(name), label)
        })
        .collect()
}

fn font_size_options(cx: &App) -> Vec<(SharedString, SharedString)> {
    config::font_size_choices(config::current(cx).font_size)
        .into_iter()
        .map(|size| {
            let label = format_font_size(size);
            (SharedString::from(label.clone()), SharedString::from(label))
        })
        .collect()
}

fn enum_options<T>(
    values: impl IntoIterator<Item = T>,
    as_str: impl Fn(T) -> &'static str,
    label: impl Fn(T) -> &'static str,
) -> Vec<(SharedString, SharedString)>
where
    T: Copy,
{
    values
        .into_iter()
        .map(|value| (SharedString::from(as_str(value)), SharedString::from(label(value))))
        .collect()
}

fn format_font_size(size: f32) -> String {
    if (size - size.round()).abs() < 0.001 {
        format!("{:.0} pt", size.round())
    } else {
        format!("{size} pt")
    }
}

fn parse_font_size(label: &str) -> Option<f32> {
    label.trim().trim_end_matches("pt").trim().parse().ok()
}

fn reveal_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "Reveal in Finder"
    } else if cfg!(windows) {
        "Show in Explorer"
    } else {
        "Show in Folder"
    }
}
