use std::{
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use gpui::{
    Bounds, ClipboardItem, Context, Corner, CursorStyle, EventEmitter, FocusHandle, InteractiveElement, IntoElement,
    KeyDownEvent, ModifiersChangedEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels,
    Render, ScrollWheelEvent, Styled, Window, anchored, canvas, div, px, rgb,
};
use libghostty_vt::{
    Terminal,
    fmt::Format,
    key, paste,
    render::{CellIterator, RenderState, RowIterator},
    selection::FormatOptions,
    selection::gesture::{DragEvent, Geometry, Gesture, PressEvent, ReleaseEvent},
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
    ClearSelection,
    Copy(flume::Sender<Option<String>>),
    Paste(String),
}

enum Event {
    Frame(Frame),
    Title(String),
    Exited,
}

pub enum SessionEvent {
    Exited,
}

pub struct Session {
    pub title: String,
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
}

#[derive(Clone, Debug)]
struct LinkMenu {
    position: gpui::Point<Pixels>,
    hit: LinkHit,
}

impl Session {
    pub fn spawn(index: usize, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (command_tx, command_rx) = flume::unbounded();
        let (event_tx, event_rx) = flume::unbounded();
        let (pty_tx, pty_rx) = flume::unbounded();

        let cell = frame::measure_cell(window);
        let cols = 80;
        let rows = 24;
        let pty = pty::spawn_shell(cols, rows, f32::from(cell.x) as u32, f32::from(cell.y) as u32, pty_tx)
            .expect("failed to spawn shell");

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

        start_emulator(pty, command_rx, event_tx, cols, rows, cell);

        let focus = cx.focus_handle();
        focus.focus(window);

        cx.spawn(async move |this, cx| {
            while let Ok(event) = event_rx.recv_async().await {
                let exited = matches!(event, Event::Exited);
                if this
                    .update(cx, |this, cx| {
                        match event {
                            Event::Frame(frame) => {
                                this.frame = frame;
                                this.recompute_hovered_link();
                            }
                            Event::Title(title) if !title.is_empty() => this.title = title,
                            Event::Title(_) => {}
                            Event::Exited => cx.emit(SessionEvent::Exited),
                        }
                        cx.notify();
                    })
                    .is_err()
                    || exited
                {
                    break;
                }
            }
        })
        .detach();

        Self {
            title: format!("Tab {}", index + 1),
            focus,
            commands: command_tx,
            frame: empty_frame(),
            last_grid: (cols, rows),
            cell_size: cell,
            content_bounds: None,
            selecting: false,
            cmd_for_links: false,
            hover_cell: None,
            hovered_link: None,
            link_menu: None,
        }
    }

    pub fn focus(&self, window: &mut Window) {
        self.focus.focus(window);
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

    pub fn paste_clipboard(&self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            crate::notify::show(cx, "Clipboard is empty");
            return;
        };
        if text.is_empty() {
            crate::notify::show(cx, "Clipboard is empty");
            return;
        }
        let _ = self.commands.send(Command::Paste(text));
    }

    fn handle_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" && self.dismiss_link_menu(cx) {
            cx.stop_propagation();
            return;
        }
        if reserved_shortcut(&event.keystroke.modifiers, &event.keystroke.key) {
            return;
        }
        if let Some(encoded) = input::encode_keystroke(&event.keystroke) {
            let _ = self.commands.send(Command::ClearSelection);
            let _ = self.commands.send(Command::Key(encoded));
            cx.stop_propagation();
        }
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
                hit.action.open(cx);
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
        if !self.selecting && !event.dragging() {
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
        self.hover_cell.and_then(|(row, col)| self.frame.link_at(row, col))
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
        menu.hit.action.open(cx);
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
        let delta = match event.delta.pixel_delta(px(theme::FONT_SIZE * theme::LINE_HEIGHT)) {
            gpui::Point { y, .. } => {
                let lines = (f32::from(y) / (theme::FONT_SIZE * theme::LINE_HEIGHT)).round() as isize;
                -lines
            }
        };
        if delta != 0 {
            let _ = self.commands.send(Command::Scroll(delta));
            cx.stop_propagation();
        }
    }

    fn sync_size(&mut self, bounds: Bounds<Pixels>, window: &mut Window) {
        let cell = frame::measure_cell(window);
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

        div()
            .id("terminal")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .bg(rgb(theme::WINDOW))
            .overflow_hidden()
            .cursor(if self.hovered_link.is_some() {
                CursorStyle::PointingHand
            } else {
                CursorStyle::IBeam
            })
            .on_key_down(cx.listener(|this, event, _window, cx| this.handle_key(event, cx)))
            .on_action(cx.listener(|this, _: &crate::Copy, _window, cx| this.copy_selection(cx)))
            .on_action(cx.listener(|this, _: &crate::Paste, _window, cx| this.paste_clipboard(cx)))
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
                            entity.update(cx, |this, _cx| this.sync_size(bounds, window));
                        }
                    },
                    move |bounds, _, window, cx| {
                        let cell = frame::measure_cell(window);
                        frame::paint(
                            &frame,
                            bounds,
                            cell,
                            &frame::terminal_font(),
                            px(theme::FONT_SIZE),
                            hovered.as_ref(),
                            window,
                            cx,
                        );
                    },
                )
                .size_full(),
            )
            .children(link_menu.map(|menu| link_context_menu(menu, cx)))
    }
}

fn link_context_menu(menu: LinkMenu, cx: &mut Context<Session>) -> impl IntoElement {
    let open_label = match menu.hit.action {
        LinkAction::OpenUrl(_) => "Open Link",
        LinkAction::OpenFolder(_) => "Open Folder",
    };

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
                    .bg(rgb(theme::TOOLTIP))
                    .border_1()
                    .border_color(rgb(theme::SIDEBAR_BORDER))
                    .shadow_md()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                    .child(link_menu_item("link-menu-copy", "Copy", cx, |this, cx| this.copy_link(cx)))
                    .child(link_menu_item("link-menu-paste", "Paste", cx, |this, cx| this.paste_from_menu(cx)))
                    .child(link_menu_item("link-menu-open", open_label, cx, |this, cx| this.open_from_menu(cx))),
            ),
        )
}

fn link_menu_item(
    id: &'static str,
    label: &'static str,
    cx: &mut Context<Session>,
    on_click: impl Fn(&mut Session, &mut Context<Session>) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .w_full()
        .px_3()
        .py_1()
        .text_sm()
        .text_color(rgb(theme::TEXT))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(theme::TAB_HOVER)))
        .child(label)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _event, _window, cx| {
                cx.stop_propagation();
                on_click(this, cx);
            }),
        )
}

fn reserved_shortcut(modifiers: &gpui::Modifiers, key: &str) -> bool {
    modifiers.platform && matches!(key, "q" | "t" | "w" | "n" | "c" | "v")
}

fn empty_frame() -> Frame {
    Frame {
        background: frame::Rgb { r: 15, g: 17, b: 21 },
        _foreground: frame::Rgb { r: 192, g: 202, b: 245 },
        rows: Vec::new(),
        cursor: None,
    }
}

fn start_emulator(
    pty: PtyIo,
    commands: flume::Receiver<Command>,
    events: flume::Sender<Event>,
    cols: u16,
    rows: u16,
    cell: gpui::Point<Pixels>,
) {
    thread::Builder::new()
        .name("libghostty".into())
        .spawn(move || {
            if let Err(error) = run_emulator(pty, commands, events, cols, rows, cell) {
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
) -> Result<(), Box<dyn std::error::Error>> {
    let grid = Arc::new(Mutex::new(SizeReportSize {
        rows,
        columns: cols,
        cell_width: f32::from(cell.x) as u32,
        cell_height: f32::from(cell.y) as u32,
    }));

    let mut terminal = Terminal::new(cols, rows)?;
    terminal.set_scrollback_max_lines(Some(2000))?;
    terminal.resize(cols, rows, f32::from(cell.x) as u32, f32::from(cell.y) as u32)?;

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
    let mut encoded = Vec::new();

    while let Ok(command) = commands.recv() {
        match command {
            Command::PtyBytes(bytes) => terminal.vt_write(&bytes),
            Command::Key(input) => {
                let _ = terminal.set_selection(None);
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
            Command::ClearSelection => {
                gesture.reset(&terminal);
                let _ = terminal.set_selection(None);
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
        }

        if let Ok(title) = terminal.title() {
            if title != last_title {
                last_title = title.to_string();
                if events.send(Event::Title(last_title.clone())).is_err() {
                    break;
                }
            }
        }

        match frame::capture(&terminal, &mut render_state, &mut row_it, &mut cell_it) {
            Ok(frame) => {
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
