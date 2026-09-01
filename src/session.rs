use std::{
    sync::{Arc, Mutex},
    thread,
};

use gpui::{
    canvas, div, px, rgb, Bounds, Context, FocusHandle, InteractiveElement, IntoElement,
    KeyDownEvent, MouseButton, ParentElement, Pixels, Render, ScrollWheelEvent, Styled, Window,
};
use libghostty_vt::{
    key,
    render::{CellIterator, RenderState, RowIterator},
    terminal::{
        ConformanceLevel, DeviceAttributeFeature, DeviceAttributes, DeviceType,
        PrimaryDeviceAttributes, ScrollViewport, SecondaryDeviceAttributes, SizeReportSize,
    },
    Terminal,
};

use crate::{
    frame::{self, Frame},
    input,
    pty::{self, PtyIo},
    theme,
};

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
}

enum Event {
    Frame(Frame),
    Title(String),
}

pub struct Session {
    pub title: String,
    focus: FocusHandle,
    commands: flume::Sender<Command>,
    frame: Frame,
    last_grid: (u16, u16),
}

impl Session {
    pub fn spawn(index: usize, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (command_tx, command_rx) = flume::unbounded();
        let (event_tx, event_rx) = flume::unbounded();
        let (pty_tx, pty_rx) = flume::unbounded();

        let cell = frame::measure_cell(window);
        let cols = 80;
        let rows = 24;
        let pty = pty::spawn_shell(
            cols,
            rows,
            f32::from(cell.x) as u32,
            f32::from(cell.y) as u32,
            pty_tx,
        )
        .expect("failed to spawn shell");

        let command_tx_pty = command_tx.clone();
        thread::Builder::new()
            .name("pty-forward".into())
            .spawn(move || {
                while let Ok(bytes) = pty_rx.recv() {
                    if command_tx_pty.send(Command::PtyBytes(bytes)).is_err() {
                        break;
                    }
                }
            })
            .expect("failed to spawn pty forwarder");

        start_emulator(pty, command_rx, event_tx, cols, rows, cell);

        let focus = cx.focus_handle();
        focus.focus(window);

        cx.spawn(async move |this, cx| {
            while let Ok(event) = event_rx.recv_async().await {
                if this
                    .update(cx, |this, cx| {
                        match event {
                            Event::Frame(frame) => this.frame = frame,
                            Event::Title(title) if !title.is_empty() => this.title = title,
                            Event::Title(_) => {}
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
            focus,
            commands: command_tx,
            frame: empty_frame(),
            last_grid: (cols, rows),
        }
    }

    pub fn focus(&self, window: &mut Window) {
        self.focus.focus(window);
    }

    fn handle_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if reserved_shortcut(&event.keystroke.modifiers, &event.keystroke.key) {
            return;
        }
        if let Some(encoded) = input::encode_keystroke(&event.keystroke) {
            let _ = self.commands.send(Command::Key(encoded));
            cx.stop_propagation();
        }
    }

    fn handle_scroll(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let delta = match event
            .delta
            .pixel_delta(px(theme::FONT_SIZE * theme::LINE_HEIGHT))
        {
            gpui::Point { y, .. } => {
                let lines =
                    (f32::from(y) / (theme::FONT_SIZE * theme::LINE_HEIGHT)).round() as isize;
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
        let pad = f32::from(theme::TERMINAL_PAD) * 2.0;
        let cols = ((f32::from(bounds.size.width) - pad) / f32::from(cell.x))
            .floor()
            .max(1.0) as u16;
        let rows = ((f32::from(bounds.size.height) - pad) / f32::from(cell.y))
            .floor()
            .max(1.0) as u16;
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

impl Render for Session {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let frame = self.frame.clone();
        let entity = cx.entity();

        div()
            .id("terminal")
            .track_focus(&self.focus)
            .size_full()
            .bg(rgb(theme::WINDOW))
            .overflow_hidden()
            .on_key_down(cx.listener(|this, event, _window, cx| this.handle_key(event, cx)))
            .on_scroll_wheel(cx.listener(|this, event, _window, cx| this.handle_scroll(event, cx)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _event, window, _cx| this.focus.focus(window)),
            )
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
                            window,
                            cx,
                        );
                    },
                )
                .size_full(),
            )
    }
}

fn reserved_shortcut(modifiers: &gpui::Modifiers, key: &str) -> bool {
    modifiers.platform && matches!(key, "q" | "t" | "w" | "n")
}

fn empty_frame() -> Frame {
    Frame {
        background: frame::Rgb {
            r: 15,
            g: 17,
            b: 21,
        },
        _foreground: frame::Rgb {
            r: 192,
            g: 202,
            b: 245,
        },
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
    terminal.resize(
        cols,
        rows,
        f32::from(cell.x) as u32,
        f32::from(cell.y) as u32,
    )?;

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
    let mut last_title = String::new();
    let mut encoded = Vec::new();

    while let Ok(command) = commands.recv() {
        match command {
            Command::PtyBytes(bytes) => terminal.vt_write(&bytes),
            Command::Key(input) => {
                encoded.clear();
                key_event
                    .set_action(key::Action::Press)
                    .set_key(input.key)
                    .set_mods(input.mods)
                    .set_consumed_mods(input.consumed)
                    .set_unshifted_codepoint(input.unshifted)
                    .set_utf8(input.utf8);
                key_encoder
                    .set_options_from_terminal(&terminal)
                    .encode_to_vec(&key_event, &mut encoded)?;
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
