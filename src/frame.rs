use gpui::{
    fill, point, px, rgb, size, Bounds, Font, FontStyle, FontWeight, Hsla, Pixels, Point,
    SharedString, TextRun, Window,
};
use libghostty_vt::{
    render::{CellIterator, RenderState, RowIterator},
    style::RgbColor,
    Terminal,
};

use crate::theme;

#[derive(Clone, Debug)]
pub struct Frame {
    pub background: Rgb,
    pub _foreground: Rgb,
    pub rows: Vec<FrameRow>,
    pub cursor: Option<CursorCell>,
}

#[derive(Clone, Debug)]
pub struct FrameRow {
    pub spans: Vec<FrameSpan>,
}

#[derive(Clone, Debug)]
pub struct FrameSpan {
    pub text: String,
    pub columns: u16,
    pub fg: Rgb,
    pub bg: Option<Rgb>,
    pub bold: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct CursorCell {
    pub x: u16,
    pub y: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub fn from_ghostty(color: RgbColor) -> Self {
        Self {
            r: color.r,
            g: color.g,
            b: color.b,
        }
    }

    pub fn to_hsla(self) -> Hsla {
        rgb(((self.r as u32) << 16) | ((self.g as u32) << 8) | self.b as u32).into()
    }
}

pub fn capture<'alloc>(
    terminal: &Terminal<'alloc, '_>,
    render_state: &mut RenderState<'alloc>,
    row_it: &mut RowIterator<'alloc>,
    cell_it: &mut CellIterator<'alloc>,
) -> libghostty_vt::error::Result<Frame> {
    let snapshot = render_state.update(terminal)?;
    let colors = snapshot.colors()?;
    let mut rows = Vec::with_capacity(snapshot.rows()? as usize);
    let mut text = String::with_capacity(16);
    let mut row_iter = row_it.update(&snapshot)?;

    while let Some(row) = row_iter.next() {
        let mut spans = Vec::new();
        let mut current: Option<FrameSpan> = None;
        let mut cell_iter = cell_it.update(row)?;

        while let Some(cell) = cell_iter.next() {
            let graphemes = cell.graphemes_len()?;
            let mut fg = cell.fg_color()?.unwrap_or(colors.foreground);
            let mut bg = cell.bg_color()?;
            let mut bold = false;

            if cell.has_styling()? {
                let style = cell.style()?;
                if style.inverse {
                    let default_bg = bg.unwrap_or(colors.background);
                    bg = Some(fg);
                    fg = default_bg;
                }
                bold = style.bold;
            }

            if graphemes == 0 {
                flush_span(&mut spans, &mut current);
                if let Some(bg) = bg {
                    spans.push(FrameSpan {
                        text: String::new(),
                        columns: 1,
                        fg: Rgb::from_ghostty(fg),
                        bg: Some(Rgb::from_ghostty(bg)),
                        bold,
                    });
                } else {
                    spans.push(FrameSpan {
                        text: String::new(),
                        columns: 1,
                        fg: Rgb::from_ghostty(colors.foreground),
                        bg: None,
                        bold: false,
                    });
                }
                continue;
            }

            text.clear();
            cell.graphemes_utf8(&mut text)?;

            let next = FrameSpan {
                text: text.clone(),
                columns: 1,
                fg: Rgb::from_ghostty(fg),
                bg: bg.map(Rgb::from_ghostty),
                bold,
            };

            match current.as_mut() {
                Some(span)
                    if span.fg == next.fg && span.bg == next.bg && span.bold == next.bold =>
                {
                    span.text.push_str(&next.text);
                    span.columns += 1;
                }
                Some(_) => {
                    flush_span(&mut spans, &mut current);
                    current = Some(next);
                }
                None => current = Some(next),
            }
        }

        flush_span(&mut spans, &mut current);
        rows.push(FrameRow { spans });
    }

    let cursor = if snapshot.cursor_visible()? {
        snapshot
            .cursor_viewport()?
            .map(|vp| CursorCell { x: vp.x, y: vp.y })
    } else {
        None
    };

    Ok(Frame {
        background: Rgb::from_ghostty(colors.background),
        _foreground: Rgb::from_ghostty(colors.foreground),
        rows,
        cursor,
    })
}

fn flush_span(spans: &mut Vec<FrameSpan>, current: &mut Option<FrameSpan>) {
    if let Some(span) = current.take() {
        spans.push(span);
    }
}

pub fn paint(
    frame: &Frame,
    bounds: Bounds<Pixels>,
    cell: Point<Pixels>,
    font: &Font,
    font_size: Pixels,
    window: &mut Window,
    cx: &mut gpui::App,
) {
    window.paint_quad(fill(bounds, frame.background.to_hsla()));

    let mut y = bounds.origin.y + theme::TERMINAL_PAD;
    for (row_idx, row) in frame.rows.iter().enumerate() {
        let mut x = bounds.origin.x + theme::TERMINAL_PAD;
        for span in &row.spans {
            let width = cell.x * span.columns as f32;
            if let Some(bg) = span.bg {
                window.paint_quad(fill(
                    Bounds {
                        origin: point(x, y),
                        size: size(width, cell.y),
                    },
                    bg.to_hsla(),
                ));
            }
            x += width;
        }

        // Draw the cursor under glyphs so the current cell stays readable
        // and the block lines up with the same grid used for text.
        if let Some(cursor) = frame.cursor.filter(|cursor| cursor.y as usize == row_idx) {
            window.paint_quad(fill(
                Bounds {
                    origin: point(
                        bounds.origin.x + theme::TERMINAL_PAD + cell.x * cursor.x as f32,
                        y,
                    ),
                    size: size(cell.x, cell.y),
                },
                rgb(theme::CURSOR),
            ));
        }

        x = bounds.origin.x + theme::TERMINAL_PAD;
        for span in &row.spans {
            let width = cell.x * span.columns as f32;
            if !span.text.is_empty() {
                let run_font = if span.bold {
                    font.clone().bold()
                } else {
                    font.clone()
                };
                // GPUI's force_width is the advance of *each* glyph, not the
                // span. Passing the full span width spaced letters apart.
                let line = window.text_system().shape_line(
                    SharedString::from(span.text.clone()),
                    font_size,
                    &[TextRun {
                        len: span.text.len(),
                        font: run_font,
                        color: span.fg.to_hsla(),
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    }],
                    Some(cell.x),
                );
                let _ = line.paint(point(x, y), cell.y, window, cx);
            }
            x += width;
        }

        y += cell.y;
    }
}

pub fn terminal_font() -> Font {
    Font {
        family: SharedString::from(theme::FONT_FAMILY),
        features: Default::default(),
        fallbacks: None,
        weight: FontWeight::NORMAL,
        style: FontStyle::Normal,
    }
}

pub fn measure_cell(window: &mut Window) -> Point<Pixels> {
    let font = terminal_font();
    let font_size = px(theme::FONT_SIZE);
    // Average a run of identical glyphs so rounding in the shaper does not
    // inflate a single "M" into a too-wide cell.
    const SAMPLE: &str = "MMMMMMMM";
    let line = window.text_system().shape_line(
        SharedString::from(SAMPLE),
        font_size,
        &[TextRun {
            len: SAMPLE.len(),
            font,
            color: rgb(theme::TEXT).into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }],
        None,
    );
    let width = (line.width / SAMPLE.len() as f32).max(px(1.0));
    point(width, px(theme::FONT_SIZE * theme::LINE_HEIGHT))
}
