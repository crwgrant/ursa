use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use gpui::{
    App, Bounds, ClipboardItem, Context, Corner, CursorStyle, EventEmitter, FocusHandle, InteractiveElement, IntoElement,
    KeyDownEvent, Keystroke, ModifiersChangedEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement,
    Pixels, Render, ScrollWheelEvent, Styled, Timer, Window, anchored, canvas, div, prelude::FluentBuilder, px, rgb,
};
use libghostty_vt::{
    Terminal,
    fmt::Format,
    key, paste,
    render::{CellIterator, RenderState, RowIterator},
    selection::FormatOptions,
    selection::gesture::{DragEvent, Geometry, Gesture, PressEvent, ReleaseEvent},
    style::{PaletteIndex, RgbColor},
    terminal::{
        ConformanceLevel, DeviceAttributeFeature, DeviceAttributes, DeviceType, Mode, Point, PointCoordinate,
        PrimaryDeviceAttributes, ScrollViewport, SecondaryDeviceAttributes, SizeReportSize,
    },
};

use crate::{
    frame::{self, Frame, LinkAction, LinkHit},
    input,
    pty::{self, PtyIo},
    theme,
};

const CURSOR_BLINK: Duration = Duration::from_millis(530);

#[derive(Clone, Copy, Debug)]
struct SelectPointer {
    pixel_x: f64,
    pixel_y: f64,
    col: u16,
    row: u32,
    cell_width: u32,
    columns: u32,
    surface_height: u32,
}

enum Command {
    PtyBytes(Vec<u8>),
    Key(input::EncodedKey),
    Resize {
        cols: u16,
        rows: u16,
        cell_width: u32,
        cell_height: u32,
    },
    Scroll(isize),
    SelectPress(SelectPointer),
    SelectDrag {
        pointer: SelectPointer,
        rectangle: bool,
    },
    SelectRelease(SelectPointer),
    Copy(flume::Sender<Option<String>>),
    Paste(String),
    ClearScreen,
    SetScrollback(u32),
    SetTheme(theme::Colors),
    SaveState {
        path: PathBuf,
        done: Option<flume::Sender<()>>,
    },
}

enum Event {
    Frame(Frame),
    Title(String),
    Pwd(crate::cwd::TerminalCwd),
    Exited,
}

pub enum SessionEvent {
    Exited,
    TitleChanged,
    CwdChanged,
}

#[derive(Clone, Default)]
pub struct TabRestore {
    pub cwd: Option<PathBuf>,
    pub snapshot: Option<Vec<u8>>,
}

impl TabRestore {
    pub fn marks_used(&self) -> bool {
        if self.snapshot.as_ref().is_some_and(|bytes| !bytes.is_empty()) {
            return true;
        }
        let Some(cwd) = crate::cwd::usable_cwd(self.cwd.as_deref()) else {
            return false;
        };
        pty::home_dir().as_deref() != Some(cwd.as_path())
    }
}

pub struct Session {
    pub title: String,
    cwd: Option<crate::cwd::TerminalCwd>,
    local_cwd: Option<PathBuf>,
    pub used: bool,
    pub exited: bool,
    pid: Option<u32>,
    focus: FocusHandle,
    commands: flume::Sender<Command>,
    frame: Frame,
    last_grid: (u16, u16),
    cell_size: gpui::Point<Pixels>,
    content_bounds: Option<Bounds<Pixels>>,
    selecting: bool,
    cmd_for_links: bool,
    hover_cell: Option<(u32, u16)>,
    hovered_link: Option<LinkHit>,
    link_menu: Option<LinkMenu>,
    cursor_on: bool,
    blink_gen: u64,
}

#[derive(Clone, Debug)]
struct LinkMenu {
    position: gpui::Point<Pixels>,
    hit: LinkHit,
}

impl Session {
    pub fn spawn(index: usize, window: &mut Window, cx: &mut Context<Self>, restore: TabRestore) -> Self {
        let (command_tx, command_rx) = flume::unbounded();
        let (event_tx, event_rx) = flume::unbounded();
        let (pty_tx, pty_rx) = flume::unbounded();

        let cell = frame::measure_cell(window, cx);
        let cols = 80;
        let rows = 24;
        let used = restore.marks_used();
        let spawn = crate::cwd::usable_cwd(restore.cwd.as_deref());
        let pty = pty::spawn_shell(cols, rows, f32::from(cell.x) as u32, f32::from(cell.y) as u32, pty_tx, spawn.as_deref())
            .expect("failed to spawn shell");
        let pid = pty.pid;

        let command_tx_pty = command_tx.clone();
        let event_tx_exit = event_tx.clone();
        thread::Builder::new()
            .name("pty-forward".into())
            .spawn(move || {
                while let Ok(bytes) = pty_rx.recv() {
                    if command_tx_pty.send(Command::PtyBytes(bytes)).is_err() {
                        break;
                    }
                }
                let _ = event_tx_exit.send(Event::Exited);
            })
            .expect("failed to spawn pty forwarder");

        start_emulator(
            pty,
            command_rx,
            event_tx,
            cols,
            rows,
            cell,
            crate::config::scrollback_lines(cx),
            theme::colors(cx),
            restore.snapshot,
        );

        let focus = cx.focus_handle().tab_stop(false);
        focus.focus(window);
        cx.on_focus_in(&focus, window, |this, _, cx| this.restart_cursor_blink(cx))
            .detach();
        cx.on_blur(&focus, window, |this, _, cx| this.stop_cursor_blink(cx)).detach();
        cx.defer_in(window, |this, _, cx| this.restart_cursor_blink(cx));

        cx.spawn(async move |this, cx| {
            while let Ok(event) = event_rx.recv_async().await {
                if this
                    .update(cx, |this, cx| {
                        match event {
                            Event::Frame(frame) => {
                                this.frame = frame;
                                this.cursor_on = true;
                                this.recompute_hovered_link();
                            }
                            Event::Title(title) if !title.is_empty() => {
                                this.title = if this.exited { format!("{title} (exited)") } else { title };
                                cx.emit(SessionEvent::TitleChanged);
                                return;
                            }
                            Event::Title(_) => return,
                            Event::Pwd(cwd) => {
                                this.remember_local_cwd();
                                if this.cwd.as_ref() != Some(&cwd) {
                                    this.cwd = Some(cwd);
                                    cx.emit(SessionEvent::CwdChanged);
                                }
                                return;
                            }
                            Event::Exited => {
                                this.mark_exited(cx);
                                cx.emit(SessionEvent::Exited);
                            }
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        Self {
            title: format!("Tab {}", index + 1),
            cwd: spawn.clone().map(crate::cwd::TerminalCwd::local),
            local_cwd: spawn,
            used,
            exited: false,
            pid,
            focus,
            commands: command_tx,
            frame: empty_frame(theme::colors(cx)),
            last_grid: (0, 0),
            cell_size: cell,
            content_bounds: None,
            selecting: false,
            cmd_for_links: false,
            hover_cell: None,
            hovered_link: None,
            link_menu: None,
            cursor_on: true,
            blink_gen: 0,
        }
    }

    pub fn focus(&self, window: &mut Window) {
        self.focus.focus(window);
    }

    fn restart_cursor_blink(&mut self, cx: &mut Context<Self>) {
        self.blink_gen = self.blink_gen.wrapping_add(1);
        self.cursor_on = true;
        let gen = self.blink_gen;
        cx.notify();
        cx.spawn(async move |this, cx| {
            loop {
                Timer::after(CURSOR_BLINK).await;
                let keep = this
                    .update(cx, |this, cx| {
                        if this.blink_gen != gen || this.exited {
                            return false;
                        }
                        this.cursor_on = !this.cursor_on;
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !keep {
                    break;
                }
            }
        })
        .detach();
    }

    fn stop_cursor_blink(&mut self, cx: &mut Context<Self>) {
        self.blink_gen = self.blink_gen.wrapping_add(1);
        self.cursor_on = true;
        cx.notify();
    }

    pub fn has_selection(&self) -> bool {
        self.frame.has_selection
    }

    pub fn copy_selection(&self, cx: &mut Context<Self>) {
        let (tx, rx) = flume::bounded(1);
        if self.commands.send(Command::Copy(tx)).is_err() {
            return;
        }
        cx.spawn(async move |this, cx| {
            let text = match rx.recv_async().await {
                Ok(Some(text)) if !text.is_empty() => text,
                _ => {
                    let _ = this.update(cx, |_, cx| crate::notify::show(cx, "Nothing to copy"));
                    return;
                }
            };
            let _ = this.update(cx, |_, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                crate::notify::show(cx, "Copied");
            });
        })
        .detach();
    }

    fn mark_exited(&mut self, cx: &mut Context<Self>) {
        if self.exited {
            return;
        }
        self.exited = true;
        const SUFFIX: &str = " (exited)";
        if !self.title.ends_with(SUFFIX) {
            self.title.push_str(SUFFIX);
            cx.emit(SessionEvent::TitleChanged);
        }
    }

    pub fn paste_clipboard(&mut self, cx: &mut Context<Self>) {
        if self.exited {
            return;
        }
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            crate::notify::show(cx, "Clipboard is empty");
            return;
        };
        if text.is_empty() {
            crate::notify::show(cx, "Clipboard is empty");
            return;
        }
        self.used = true;
        let _ = self.commands.send(Command::Paste(text));
    }

    pub fn clear_screen(&mut self) {
        if self.exited {
            return;
        }
        self.used = true;
        let _ = self.commands.send(Command::ClearScreen);
    }

    pub fn spawn_cwd(&self) -> Option<PathBuf> {
        crate::cwd::usable_cwd(self.local_cwd.as_deref()).or_else(|| crate::cwd::usable_local_dir(self.cwd.as_ref()))
    }

    pub fn cwd_is_remote(&self) -> bool {
        self.cwd.as_ref().is_some_and(crate::cwd::TerminalCwd::is_remote)
    }

    pub fn refresh_cwd(&mut self) {
        self.remember_local_cwd();
        if self.cwd.as_ref().is_some_and(crate::cwd::TerminalCwd::is_remote) {
            return;
        }
        if let Some(path) = self.local_cwd.clone() {
            self.cwd = Some(crate::cwd::TerminalCwd::local(path));
        }
    }

    fn remember_local_cwd(&mut self) {
        let Some(pid) = self.pid else {
            return;
        };
        if let Some(path) = crate::cwd::process_cwd(pid) {
            self.local_cwd = Some(path);
        }
    }

    fn can_open(&self, action: &LinkAction) -> bool {
        match action {
            LinkAction::OpenUrl(_) => true,
            LinkAction::OpenFolder(_) => !self.cwd_is_remote(),
        }
    }

    pub fn request_save_state(&self, path: PathBuf) {
        let _ = self.commands.send(Command::SaveState { path, done: None });
    }

    pub fn flush_state(&self, path: PathBuf) {
        let (tx, rx) = flume::bounded(1);
        if self.commands.send(Command::SaveState { path, done: Some(tx) }).is_err() {
            return;
        }
        let _ = rx.recv_timeout(Duration::from_millis(200));
    }

    pub fn apply_config(&mut self, cx: &mut Context<Self>) {
        self.last_grid = (0, 0);
        let _ = self
            .commands
            .send(Command::SetScrollback(crate::config::scrollback_lines(cx)));
        let _ = self.commands.send(Command::SetTheme(theme::colors(cx)));
        cx.notify();
    }

    fn handle_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" && self.dismiss_link_menu(cx) {
            cx.stop_propagation();
            return;
        }
        if self.send_keystroke(&event.keystroke, cx) {
            cx.stop_propagation();
        }
    }

    pub fn send_keystroke(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if reserved_shortcut(&keystroke.modifiers, &keystroke.key, cx) {
            return false;
        }
        if self.exited {
            return true;
        }
        let Some(encoded) = input::encode_keystroke(keystroke) else {
            return false;
        };
        self.used = true;
        self.restart_cursor_blink(cx);
        let _ = self.commands.send(Command::Key(encoded));
        true
    }

    fn handle_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window);
        if event.button != MouseButton::Left {
            return;
        }
        self.dismiss_link_menu(cx);
        if event.modifiers.platform {
            if let Some(hit) = self
                .pointer_at(event.position)
                .and_then(|pointer| self.frame.link_at(pointer.row, pointer.col))
            {
                if self.can_open(&hit.action) {
                    hit.action.open(cx);
                }
                cx.stop_propagation();
                return;
            }
        }
        if let Some(pointer) = self.pointer_at(event.position) {
            self.selecting = true;
            let _ = self.commands.send(Command::SelectPress(pointer));
            cx.stop_propagation();
        }
    }

    fn handle_mouse_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        self.update_link_hover(event.position, event.modifiers.platform, cx);
        if !self.selecting {
            return;
        }
        if let Some(pointer) = self.pointer_at(event.position) {
            self.selecting = true;
            let _ = self.commands.send(Command::SelectDrag {
                pointer,
                rectangle: event.modifiers.alt,
            });
            cx.stop_propagation();
        }
    }

    fn handle_modifiers(&mut self, event: &ModifiersChangedEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.update_link_hover(window.mouse_position(), event.modifiers.platform, cx);
    }

    fn update_link_hover(&mut self, position: gpui::Point<Pixels>, cmd: bool, cx: &mut Context<Self>) {
        self.cmd_for_links = cmd;
        self.hover_cell = self.pointer_at(position).map(|pointer| (pointer.row, pointer.col));
        let hit = self.current_link_hit();
        if hit != self.hovered_link {
            self.hovered_link = hit;
            cx.notify();
        }
    }

    fn recompute_hovered_link(&mut self) {
        self.hovered_link = self.current_link_hit();
    }

    fn current_link_hit(&self) -> Option<LinkHit> {
        if let Some(menu) = &self.link_menu {
            return Some(menu.hit.clone());
        }
        if !self.cmd_for_links {
            return None;
        }
        self.hover_cell
            .and_then(|(row, col)| self.frame.link_at(row, col))
            .filter(|hit| self.can_open(&hit.action))
    }

    fn handle_right_click(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window);
        if let Some(hit) = self
            .pointer_at(event.position)
            .and_then(|pointer| self.frame.link_at(pointer.row, pointer.col))
        {
            self.link_menu = Some(LinkMenu {
                position: event.position,
                hit: hit.clone(),
            });
            self.hovered_link = Some(hit);
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if self.dismiss_link_menu(cx) {
            cx.stop_propagation();
        }
    }

    fn dismiss_link_menu(&mut self, cx: &mut Context<Self>) -> bool {
        if self.link_menu.take().is_none() {
            return false;
        }
        self.hovered_link = self.current_link_hit();
        cx.notify();
        true
    }

    fn copy_link(&mut self, cx: &mut Context<Self>) {
        let Some(menu) = self.link_menu.take() else {
            return;
        };
        let text = menu.hit.action.clipboard_text();
        if text.is_empty() {
            crate::notify::show(cx, "Nothing to copy");
        } else {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            crate::notify::show(cx, "Copied");
        }
        self.hovered_link = self.current_link_hit();
        cx.notify();
    }

    fn paste_from_menu(&mut self, cx: &mut Context<Self>) {
        self.dismiss_link_menu(cx);
        self.paste_clipboard(cx);
    }

    fn open_from_menu(&mut self, cx: &mut Context<Self>) {
        let Some(menu) = self.link_menu.take() else {
            return;
        };
        if self.can_open(&menu.hit.action) {
            menu.hit.action.open(cx);
        }
        self.hovered_link = self.current_link_hit();
        cx.notify();
    }

    fn handle_mouse_up(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        if event.button != MouseButton::Left {
            return;
        }
        self.selecting = false;
        if let Some(pointer) = self.pointer_at(event.position) {
            let _ = self.commands.send(Command::SelectRelease(pointer));
        }
        cx.stop_propagation();
    }

    fn pointer_at(&self, position: gpui::Point<Pixels>) -> Option<SelectPointer> {
        let bounds = self.content_bounds?;
        let pad = f32::from(theme::TERMINAL_PAD);
        let cell_w = f32::from(self.cell_size.x).max(1.0);
        let cell_h = f32::from(self.cell_size.y).max(1.0);
        let (cols, rows) = self.last_grid;
        let local_x = f32::from(position.x - bounds.origin.x);
        let local_y = f32::from(position.y - bounds.origin.y);
        let col = ((local_x - pad) / cell_w).floor().clamp(0.0, cols.saturating_sub(1) as f32) as u16;
        let row = ((local_y - pad) / cell_h).floor().clamp(0.0, rows.saturating_sub(1) as f32) as u32;
        Some(SelectPointer {
            pixel_x: local_x as f64,
            pixel_y: local_y as f64,
            col,
            row,
            cell_width: cell_w as u32,
            columns: cols as u32,
            surface_height: f32::from(bounds.size.height) as u32,
        })
    }

    fn handle_scroll(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let line = crate::config::font_size(cx) * theme::LINE_HEIGHT;
        let delta = match event.delta.pixel_delta(px(line)) {
            gpui::Point { y, .. } => {
                let lines = (f32::from(y) / line).round() as isize;
                -lines
            }
        };
        if delta != 0 {
            let _ = self.commands.send(Command::Scroll(delta));
            cx.stop_propagation();
        }
    }

    fn sync_size(&mut self, bounds: Bounds<Pixels>, window: &mut Window, cx: &App) {
        let cell = frame::measure_cell(window, cx);
        self.cell_size = cell;
        self.content_bounds = Some(bounds);
        let pad = f32::from(theme::TERMINAL_PAD) * 2.0;
        let cols = ((f32::from(bounds.size.width) - pad) / f32::from(cell.x)).floor().max(1.0) as u16;
        let rows = ((f32::from(bounds.size.height) - pad) / f32::from(cell.y)).floor().max(1.0) as u16;
        if (cols, rows) != self.last_grid {
            self.last_grid = (cols, rows);
            let _ = self.commands.send(Command::Resize {
                cols,
                rows,
                cell_width: f32::from(cell.x) as u32,
                cell_height: f32::from(cell.y) as u32,
            });
        }
    }
}

impl EventEmitter<SessionEvent> for Session {}

impl Render for Session {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let frame = self.frame.clone();
        let hovered = self.hovered_link.clone();
        let link_menu = self.link_menu.clone();
        let entity = cx.entity();
        let colors = theme::colors(cx);
        let focus = self.focus.clone();
        let cursor_on = self.cursor_on;
        let cell = self.cell_size;

        div()
            .id(("terminal", cx.entity_id().as_u64() as usize))
            .track_focus(&self.focus)
            .tab_stop(false)
            .relative()
            .size_full()
            .bg(rgb(colors.term_bg))
            .overflow_hidden()
            .cursor(if self.hovered_link.is_some() {
                CursorStyle::PointingHand
            } else {
                CursorStyle::IBeam
            })
            .on_key_down(cx.listener(|this, event, _window, cx| this.handle_key(event, cx)))
            .when(self.frame.has_selection, |el| {
                el.on_action(cx.listener(|this, _: &crate::Copy, _window, cx| this.copy_selection(cx)))
            })
            .when(clipboard_has_text(cx), |el| {
                el.on_action(cx.listener(|this, _: &crate::Paste, _window, cx| this.paste_clipboard(cx)))
            })
            .on_action(cx.listener(|this, _: &crate::ClearScreen, _window, _cx| this.clear_screen()))
            .on_modifiers_changed(cx.listener(|this, event, window, cx| this.handle_modifiers(event, window, cx)))
            .on_scroll_wheel(cx.listener(|this, event, _window, cx| this.handle_scroll(event, cx)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event, window, cx| this.handle_mouse_down(event, window, cx)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event, window, cx| this.handle_right_click(event, window, cx)),
            )
            .on_mouse_move(cx.listener(|this, event, _window, cx| this.handle_mouse_move(event, cx)))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, event, _window, cx| this.handle_mouse_up(event, cx)))
            .child(
                canvas(
                    {
                        let entity = entity.clone();
                        move |bounds, window, cx| {
                            entity.update(cx, |this, cx| this.sync_size(bounds, window, cx));
                        }
                    },
                    move |bounds, _, window, cx| {
                        frame::paint(
                            &frame,
                            bounds,
                            cell,
                            &frame::terminal_font(cx),
                            px(crate::config::font_size(cx)),
                            hovered.as_ref(),
                            focus.is_focused(window) && cursor_on,
                            window,
                            cx,
                        );
                    },
                )
                .size_full(),
            )
            .when(self.exited, |el| el.child(exited_banner(colors)))
            .children(link_menu.map(|menu| {
                let can_open = self.can_open(&menu.hit.action);
                link_context_menu(menu, can_open, cx)
            }))
    }
}

fn link_context_menu(menu: LinkMenu, can_open: bool, cx: &mut Context<Session>) -> impl IntoElement {
    let open_label = match menu.hit.action {
        LinkAction::OpenUrl(_) => "Open Link",
        LinkAction::OpenFolder(_) => "Open Folder",
    };

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
                this.dismiss_link_menu(cx);
                cx.stop_propagation();
            }),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(|this, event, window, cx| this.handle_right_click(event, window, cx)),
        )
        .child(
            anchored().position(menu.position).anchor(Corner::TopLeft).child(
                div()
                    .occlude()
                    .min_w(px(148.0))
                    .py_1()
                    .rounded_md()
                    .bg(rgb(colors.tooltip))
                    .border_1()
                    .border_color(rgb(colors.sidebar_border))
                    .shadow_md()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                    .child(link_menu_item("link-menu-copy", "Copy", cx, |this, cx| this.copy_link(cx)))
                    .child(link_menu_item("link-menu-paste", "Paste", cx, |this, cx| this.paste_from_menu(cx)))
                    .when(can_open, |el| {
                        el.child(link_menu_item("link-menu-open", open_label, cx, |this, cx| this.open_from_menu(cx)))
                    }),
            ),
        )
}

fn link_menu_item(
    id: &'static str,
    label: &'static str,
    cx: &mut Context<Session>,
    on_click: impl Fn(&mut Session, &mut Context<Session>) + 'static,
) -> impl IntoElement {
    let colors = theme::colors(cx);
    div()
        .id(id)
        .w_full()
        .px_3()
        .py_1()
        .text_sm()
        .text_color(rgb(colors.text))
        .cursor_pointer()
        .hover(move |style| style.bg(rgb(colors.tab_hover)))
        .child(label)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _event, _window, cx| {
                cx.stop_propagation();
                on_click(this, cx);
            }),
        )
}

pub fn clipboard_has_text(cx: &App) -> bool {
    cx.read_from_clipboard()
        .and_then(|item| item.text())
        .is_some_and(|text| !text.is_empty())
}

fn reserved_shortcut(modifiers: &gpui::Modifiers, key: &str, cx: &App) -> bool {
    let digit = matches!(key, "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9");
    if modifiers.platform && (matches!(key, "q" | "t" | "w" | "n" | "c" | "v" | "k" | "d" | "[" | "]" | ",") || digit) {
        return true;
    }
    if modifiers.control && modifiers.shift && (matches!(key, "t" | "w" | "c" | "v" | "d" | "[" | "]") || digit) {
        return true;
    }
    if modifiers.control && modifiers.alt && matches!(key, "t" | "w" | "d") {
        return true;
    }
    if modifiers.control && key == "," {
        return true;
    }
    modifiers.control && digit && crate::config::sessions_enabled(cx)
}

fn exited_banner(colors: theme::Colors) -> impl IntoElement {
    div()
        .absolute()
        .bottom_0()
        .left_0()
        .right_0()
        .px_3()
        .py_2()
        .bg(rgb(colors.window))
        .border_t_1()
        .border_color(rgb(colors.sidebar_border))
        .text_sm()
        .text_color(rgb(colors.text_dim))
        .child("Shell exited. This tab is still open.")
}

fn empty_frame(colors: theme::Colors) -> Frame {
    let (br, bg, bb) = theme::Colors::rgb_parts(colors.term_bg);
    let (fr, fg, fb) = theme::Colors::rgb_parts(colors.term_fg);
    Frame {
        background: frame::Rgb { r: br, g: bg, b: bb },
        _foreground: frame::Rgb { r: fr, g: fg, b: fb },
        rows: Vec::new(),
        cursor: None,
        has_selection: false,
    }
}

fn ghostty_rgb(color: u32) -> RgbColor {
    let (r, g, b) = theme::Colors::rgb_parts(color);
    RgbColor { r, g, b }
}

fn apply_terminal_theme(terminal: &mut Terminal, colors: theme::Colors) -> Result<(), Box<dyn std::error::Error>> {
    terminal
        .set_default_fg_color(Some(ghostty_rgb(colors.term_fg)))?
        .set_default_bg_color(Some(ghostty_rgb(colors.term_bg)))?
        .set_default_cursor_color(Some(ghostty_rgb(colors.cursor)))?
        .set_default_cursor_blink(Some(true))?;

    let mut palette = terminal.default_color_palette()?;
    let indexes = [
        PaletteIndex::BLACK,
        PaletteIndex::RED,
        PaletteIndex::GREEN,
        PaletteIndex::YELLOW,
        PaletteIndex::BLUE,
        PaletteIndex::MAGENTA,
        PaletteIndex::CYAN,
        PaletteIndex::WHITE,
        PaletteIndex::BRIGHT_BLACK,
        PaletteIndex::BRIGHT_RED,
        PaletteIndex::BRIGHT_GREEN,
        PaletteIndex::BRIGHT_YELLOW,
        PaletteIndex::BRIGHT_BLUE,
        PaletteIndex::BRIGHT_MAGENTA,
        PaletteIndex::BRIGHT_CYAN,
        PaletteIndex::BRIGHT_WHITE,
    ];
    for (index, color) in indexes.into_iter().zip(colors.ansi) {
        palette.set(index, ghostty_rgb(color));
    }
    terminal.set_default_color_palette(Some(palette))?;
    Ok(())
}

fn restore_terminal(
    snapshot: Option<&[u8]>,
    cols: u16,
    rows: u16,
) -> Result<(Terminal<'static, 'static>, bool), Box<dyn std::error::Error>> {
    if let Some(bytes) = snapshot {
        if !bytes.is_empty() {
            if let Ok(terminal) = libghostty_vt::snapshot::Decoder::new_buf(bytes).and_then(|decoder| decoder.decode()) {
                return Ok((terminal, true));
            }
        }
    }
    Ok((Terminal::new(cols, rows)?, false))
}

fn terminal_cwd(terminal: &Terminal, pid: Option<u32>) -> Option<crate::cwd::TerminalCwd> {
    if let Ok(raw) = terminal.pwd() {
        if let Some(cwd) = crate::cwd::parse_pwd(raw) {
            return Some(cwd);
        }
    }
    pid.and_then(crate::cwd::process_cwd).map(crate::cwd::TerminalCwd::local)
}

fn start_emulator(
    pty: PtyIo,
    commands: flume::Receiver<Command>,
    events: flume::Sender<Event>,
    cols: u16,
    rows: u16,
    cell: gpui::Point<Pixels>,
    scrollback_lines: u32,
    colors: theme::Colors,
    snapshot: Option<Vec<u8>>,
) {
    thread::Builder::new()
        .name("libghostty".into())
        .spawn(move || {
            if let Err(error) = run_emulator(pty, commands, events, cols, rows, cell, scrollback_lines, colors, snapshot) {
                eprintln!("terminal thread exited: {error}");
            }
        })
        .expect("failed to start libghostty thread");
}

fn run_emulator(
    pty: PtyIo,
    commands: flume::Receiver<Command>,
    events: flume::Sender<Event>,
    cols: u16,
    rows: u16,
    cell: gpui::Point<Pixels>,
    scrollback_lines: u32,
    colors: theme::Colors,
    snapshot: Option<Vec<u8>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let grid = Arc::new(Mutex::new(SizeReportSize {
        rows,
        columns: cols,
        cell_width: f32::from(cell.x) as u32,
        cell_height: f32::from(cell.y) as u32,
    }));

    let (mut terminal, restored) = restore_terminal(snapshot.as_deref(), cols, rows)?;
    let _ = terminal.set_continuation_max_bytes(64 * 1024);
    terminal.set_scrollback_max_lines(Some(scrollback_lines as usize))?;
    terminal.resize(cols, rows, f32::from(cell.x) as u32, f32::from(cell.y) as u32)?;
    apply_terminal_theme(&mut terminal, colors)?;
    if restored {
        // Snapshot leaves the cursor on the old prompt. A new shell — zsh with
        // PROMPT_SP in particular — then prints a reverse-video '%' for a
        // "partial line" that never ended in a newline.
        terminal.vt_write(b"\r\n");
    }
    let pid = pty.pid;

    let writer = pty.writer.clone();
    terminal.on_pty_write(move |_term, data| {
        pty::write_pty(&writer, data);
    })?;
    terminal.on_size({
        let grid = grid.clone();
        move |_term| Some(*grid.lock().unwrap())
    })?;
    terminal.on_device_attributes(|_term| {
        Some(DeviceAttributes {
            primary: PrimaryDeviceAttributes::new(
                ConformanceLevel::VT220,
                &[
                    DeviceAttributeFeature::COLUMNS_132,
                    DeviceAttributeFeature::SELECTIVE_ERASE,
                    DeviceAttributeFeature::ANSI_COLOR,
                ],
            ),
            secondary: SecondaryDeviceAttributes {
                device_type: DeviceType::VT220,
                firmware_version: 1,
                rom_cartridge: 0,
            },
            tertiary: Default::default(),
        })
    })?;
    terminal.on_xtversion(|_term| Some("ghostterm"))?;

    let mut key_encoder = key::Encoder::new()?;
    let mut key_event = key::Event::new()?;
    let mut render_state = RenderState::new()?;
    let mut row_it = RowIterator::new()?;
    let mut cell_it = CellIterator::new()?;
    let mut gesture = Gesture::new()?;
    let mut press_event = PressEvent::new()?;
    press_event.set_repeat_interval(Duration::from_millis(500))?;
    let mut drag_event = DragEvent::new()?;
    let mut release_event = ReleaseEvent::new()?;
    let started = Instant::now();
    let mut last_title = String::new();
    let mut last_cwd: Option<crate::cwd::TerminalCwd> = None;
    let mut encoded = Vec::new();
    let mut painted_selection = false;

    while let Ok(command) = commands.recv() {
        let mut refresh_cwd = false;
        let mut needs_frame = true;
        match command {
            Command::PtyBytes(bytes) => {
                terminal.vt_write(&bytes);
                refresh_cwd = true;
            }
            Command::Key(input) => {
                needs_frame = painted_selection;
                if painted_selection {
                    let _ = terminal.set_selection(None);
                    painted_selection = false;
                }
                encoded.clear();
                if let Some(raw) = input.raw.as_deref() {
                    encoded.extend_from_slice(raw);
                } else {
                    key_event
                        .set_action(key::Action::Press)
                        .set_key(input.key)
                        .set_mods(input.mods)
                        .set_consumed_mods(input.consumed)
                        .set_unshifted_codepoint(input.unshifted)
                        .set_utf8(input.utf8.clone());
                    key_encoder
                        .set_options_from_terminal(&terminal)
                        .encode_to_vec(&key_event, &mut encoded)?;
                    if encoded.is_empty() {
                        if let Some(text) = input.utf8.as_deref() {
                            encoded.extend_from_slice(text.as_bytes());
                        }
                    }
                }
                pty::write_pty(&pty.writer, &encoded);
            }
            Command::Resize {
                cols,
                rows,
                cell_width,
                cell_height,
            } => {
                if let Ok(mut size) = grid.lock() {
                    size.columns = cols;
                    size.rows = rows;
                    size.cell_width = cell_width;
                    size.cell_height = cell_height;
                }
                terminal.resize(cols, rows, cell_width, cell_height)?;
                pty::resize_pty(&pty.master, cols, rows, cell_width, cell_height);
            }
            Command::Scroll(delta) => {
                terminal.scroll_viewport(ScrollViewport::Delta(delta));
            }
            Command::SelectPress(pointer) => {
                apply_select_press(&mut terminal, &mut gesture, &mut press_event, pointer, started.elapsed())?;
            }
            Command::SelectDrag { pointer, rectangle } => {
                apply_select_drag(&mut terminal, &mut gesture, &mut drag_event, pointer, rectangle)?;
            }
            Command::SelectRelease(pointer) => {
                apply_select_release(&mut terminal, &mut gesture, &mut release_event, pointer)?;
            }
            Command::Copy(reply) => {
                let text = selection_text(&terminal);
                let _ = reply.send(text);
            }
            Command::Paste(text) => {
                gesture.reset(&terminal);
                let _ = terminal.set_selection(None);
                write_paste(&pty, &terminal, text);
            }
            Command::ClearScreen => {
                gesture.reset(&terminal);
                let _ = terminal.set_selection(None);
                terminal.scroll_viewport(ScrollViewport::Bottom);
                // CSI 3J drops xterm scrollback; CSI H/2J clears the active
                // display. Form feed asks the shell (or vim) to redraw.
                terminal.vt_write(b"\x1b[3J\x1b[H\x1b[2J");
                pty::write_pty(&pty.writer, b"\x0c");
            }
            Command::SetScrollback(lines) => {
                terminal.set_scrollback_max_lines(Some(lines as usize))?;
            }
            Command::SetTheme(colors) => {
                apply_terminal_theme(&mut terminal, colors)?;
            }
            Command::SaveState { path, done } => {
                if let Ok(Some(bytes)) = terminal.encode_snapshot_alloc(None) {
                    crate::config::write_tab_snapshot(&path, &bytes);
                }
                refresh_cwd = true;
                needs_frame = false;
                if let Some(done) = done {
                    let _ = done.send(());
                }
            }
        }

        if refresh_cwd {
            if let Some(cwd) = terminal_cwd(&terminal, pid) {
                if last_cwd.as_ref() != Some(&cwd) {
                    last_cwd = Some(cwd.clone());
                    if events.send(Event::Pwd(cwd)).is_err() {
                        break;
                    }
                }
            }
        }

        if let Ok(title) = terminal.title() {
            if title != last_title {
                last_title = title.to_string();
                if events.send(Event::Title(last_title.clone())).is_err() {
                    break;
                }
            }
        }

        if !needs_frame {
            continue;
        }

        match frame::capture(&terminal, &mut render_state, &mut row_it, &mut cell_it) {
            Ok(frame) => {
                painted_selection = frame.has_selection;
                if events.send(Event::Frame(frame)).is_err() {
                    break;
                }
            }
            Err(error) => eprintln!("failed to capture terminal frame: {error}"),
        }
    }

    Ok(())
}

fn viewport_ref<'t>(terminal: &'t Terminal<'_, '_>, pointer: SelectPointer) -> Option<libghostty_vt::screen::GridRef<'t>> {
    terminal
        .grid_ref(Point::Viewport(PointCoordinate {
            x: pointer.col,
            y: pointer.row,
        }))
        .ok()
}

fn apply_select_press(
    terminal: &mut Terminal,
    gesture: &mut Gesture,
    press_event: &mut PressEvent,
    pointer: SelectPointer,
    elapsed: Duration,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(grid_ref) = viewport_ref(terminal, pointer) else {
        return Ok(());
    };
    let selection = press_event
        .set_repeat_distance(pointer.cell_width as f64)?
        .set_time(elapsed)?
        .set_position(pointer.pixel_x, pointer.pixel_y)?
        .apply(gesture, terminal, grid_ref)?;
    terminal.set_selection(selection.as_ref())?;
    Ok(())
}

fn apply_select_drag(
    terminal: &mut Terminal,
    gesture: &mut Gesture,
    drag_event: &mut DragEvent,
    pointer: SelectPointer,
    rectangle: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(grid_ref) = viewport_ref(terminal, pointer) else {
        return Ok(());
    };
    let geometry = Geometry {
        columns: pointer.columns,
        cell_width: pointer.cell_width,
        padding_left: f32::from(theme::TERMINAL_PAD) as u32,
        screen_height: pointer.surface_height.max(1),
    };
    let selection = drag_event
        .set_rectangle(rectangle)?
        .set_position(pointer.pixel_x, pointer.pixel_y)?
        .apply(gesture, terminal, grid_ref, geometry)?;
    terminal.set_selection(selection.as_ref())?;
    Ok(())
}

fn apply_select_release(
    terminal: &mut Terminal,
    gesture: &mut Gesture,
    release_event: &mut ReleaseEvent,
    pointer: SelectPointer,
) -> Result<(), Box<dyn std::error::Error>> {
    let grid_ref = viewport_ref(terminal, pointer);
    release_event.apply(gesture, terminal, grid_ref)?;
    Ok(())
}

fn selection_text(terminal: &Terminal) -> Option<String> {
    let options = FormatOptions::new()
        .with_emit_format(Format::Plain)
        .with_unwrap(true)
        .with_trim(true);
    let bytes = terminal.format_selection_alloc(None, options).ok()??;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if text.is_empty() { None } else { Some(text) }
}

fn write_paste(pty: &PtyIo, terminal: &Terminal, text: String) {
    let bracketed = terminal.mode(Mode::BRACKETED_PASTE).unwrap_or(false);
    let original = text.into_bytes();
    let mut data = original.clone();
    let mut buf = vec![0u8; data.len().saturating_add(32)];
    loop {
        match paste::encode(&mut data, bracketed, &mut buf) {
            Ok(len) => {
                pty::write_pty(&pty.writer, &buf[..len]);
                break;
            }
            Err(libghostty_vt::Error::OutOfSpace { required }) if required > buf.len() => {
                buf.resize(required, 0);
                data.clone_from(&original);
            }
            Err(_) => {
                let fallback: Vec<u8> = original
                    .iter()
                    .map(|&byte| if byte == b'\n' { b'\r' } else { byte })
                    .collect();
                pty::write_pty(&pty.writer, &fallback);
                break;
            }
        }
    }
}
