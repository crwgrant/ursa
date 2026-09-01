use gpui::{
    fill, point, px, rgb, size, Bounds, Font, FontStyle, FontWeight, Hsla, Pixels, Point,
    SharedString, TextRun, Window,
};
use libghostty_vt::{
    error::Error,
    render::{CellIterator, RenderState, RowIterator},
    style::RgbColor,
    terminal::{Point as TermPoint, PointCoordinate},
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
    pub wrapped: bool,
}

#[derive(Clone, Debug)]
pub struct FrameSpan {
    pub text: String,
    pub columns: u16,
    pub fg: Rgb,
    pub bg: Option<Rgb>,
    pub bold: bool,
    pub link: Option<String>,
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
    let mut row_idx = 0u32;

    while let Some(row) = row_iter.next() {
        let wrapped = row.raw_row()?.is_wrapped().unwrap_or(false);
        let mut spans = Vec::new();
        let mut current: Option<FrameSpan> = None;
        let mut cell_iter = cell_it.update(row)?;
        let mut col = 0u16;

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

            if cell.is_selected()? {
                let selected_bg = bg.unwrap_or(colors.background);
                bg = Some(fg);
                fg = selected_bg;
            }

            let link = hyperlink_uri(terminal, col, row_idx, cell);

            if graphemes == 0 {
                flush_span(&mut spans, &mut current);
                spans.push(FrameSpan {
                    text: String::new(),
                    columns: 1,
                    fg: Rgb::from_ghostty(fg),
                    bg: bg.map(Rgb::from_ghostty),
                    bold,
                    link,
                });
                col = col.saturating_add(1);
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
                link,
            };

            match current.as_mut() {
                Some(span)
                    if span.fg == next.fg
                        && span.bg == next.bg
                        && span.bold == next.bold
                        && span.link == next.link =>
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
            col = col.saturating_add(1);
        }

        flush_span(&mut spans, &mut current);
        rows.push(FrameRow { spans, wrapped });
        row_idx += 1;
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

fn hyperlink_uri(
    terminal: &Terminal<'_, '_>,
    col: u16,
    row: u32,
    cell: &libghostty_vt::render::CellIteration<'_, '_>,
) -> Option<String> {
    let raw = cell.raw_cell().ok()?;
    if !raw.has_hyperlink().ok()? {
        return None;
    }
    let grid_ref = terminal
        .grid_ref(TermPoint::Viewport(PointCoordinate { x: col, y: row }))
        .ok()?;
    let mut buf = vec![0u8; 256];
    loop {
        match grid_ref.hyperlink_uri(&mut buf) {
            Ok(0) => return None,
            Ok(len) => return String::from_utf8(buf[..len].to_vec()).ok(),
            Err(Error::OutOfSpace { required }) if required > buf.len() => {
                buf.resize(required.max(buf.len().saturating_mul(2)), 0);
            }
            Err(_) => return None,
        }
    }
}

impl Frame {
    /// Website URL under a viewport cell, if Cmd+click should open it.
    pub fn url_at(&self, row: u32, col: u16) -> Option<String> {
        let row = row as usize;
        if let Some(uri) = hyperlink_at_cell(self, row, col).filter(|uri| is_web_url(uri)) {
            return Some(uri);
        }
        let (text, at) = paragraph_text_at(self, row, col)?;
        find_web_url_at(&text, at)
    }
}

fn hyperlink_at_cell(frame: &Frame, row: usize, col: u16) -> Option<String> {
    let row = frame.rows.get(row)?;
    let mut x = 0u16;
    for span in &row.spans {
        if col < x.saturating_add(span.columns) {
            return span.link.clone();
        }
        x = x.saturating_add(span.columns);
    }
    None
}

fn paragraph_text_at(frame: &Frame, row: usize, col: u16) -> Option<(String, usize)> {
    if row >= frame.rows.len() {
        return None;
    }
    let (start, end) = wrapped_range(&frame.rows, row);
    let mut text = String::new();
    let mut click_at = None;
    for r in start..=end {
        let mut x = 0u16;
        for span in &frame.rows[r].spans {
            let span_end = x.saturating_add(span.columns);
            if r == row && col >= x && col < span_end && click_at.is_none() {
                if span.text.is_empty() {
                    click_at = Some(text.len().saturating_sub(1));
                } else {
                    let local = (col - x) as usize;
                    click_at = Some(text.len() + byte_offset_for_column(&span.text, local));
                }
            }
            text.push_str(&span.text);
            x = span_end;
        }
    }
    let at = click_at.filter(|_| !text.is_empty())?;
    let at = at.min(text.len().saturating_sub(1));
    Some((text, at))
}

fn wrapped_range(rows: &[FrameRow], row: usize) -> (usize, usize) {
    let mut start = row;
    while start > 0 && rows[start - 1].wrapped {
        start -= 1;
    }
    let mut end = row;
    while end + 1 < rows.len() && rows[end].wrapped {
        end += 1;
    }
    (start, end)
}

fn byte_offset_for_column(text: &str, col: usize) -> usize {
    text.char_indices()
        .nth(col)
        .map(|(i, _)| i)
        .unwrap_or_else(|| text.len().saturating_sub(1))
}

fn find_web_url_at(text: &str, at: usize) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    let at = at.min(text.len().saturating_sub(1));
    let lower = text.to_ascii_lowercase();
    for prefix in ["https://", "http://", "www."] {
        let mut search = 0;
        while let Some(rel) = lower[search..].find(prefix) {
            let start = search + rel;
            let after = start + prefix.len();
            let extra = lower.as_bytes()[after..]
                .iter()
                .copied()
                .take_while(|&b| is_url_byte(b))
                .count();
            let end = trim_url_end(&lower, start, after + extra);
            if end > after && at >= start && at < end {
                let mut url = text[start..end].to_string();
                if url.to_ascii_lowercase().starts_with("www.") {
                    url.insert_str(0, "https://");
                }
                return is_web_url(&url).then_some(url);
            }
            search = start + prefix.len();
        }
    }
    None
}

fn is_url_byte(byte: u8) -> bool {
    matches!(
        byte,
        b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'.'
            | b'_'
            | b'~'
            | b':'
            | b'/'
            | b'?'
            | b'#'
            | b'['
            | b']'
            | b'@'
            | b'!'
            | b'$'
            | b'&'
            | b'\''
            | b'('
            | b')'
            | b'*'
            | b'+'
            | b','
            | b';'
            | b'='
            | b'%'
    )
}

fn trim_url_end(lower: &str, start: usize, mut end: usize) -> usize {
    let bytes = lower.as_bytes();
    while end > start {
        match bytes[end - 1] {
            b'.' | b',' | b';' | b':' | b'?' | b'!' => end -= 1,
            b')' if unmatched(&bytes[start..end], b'(', b')') => end -= 1,
            b']' if unmatched(&bytes[start..end], b'[', b']') => end -= 1,
            b'}' if unmatched(&bytes[start..end], b'{', b'}') => end -= 1,
            b'>' if unmatched(&bytes[start..end], b'<', b'>') => end -= 1,
            b'\'' | b'"' => end -= 1,
            _ => break,
        }
    }
    end
}

fn unmatched(bytes: &[u8], open: u8, close: u8) -> bool {
    let mut depth = 0i32;
    for &byte in bytes {
        if byte == open {
            depth += 1;
        } else if byte == close {
            depth -= 1;
        }
    }
    depth < 0
}

fn is_web_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    (lower.starts_with("https://") || lower.starts_with("http://"))
        && lower.len() > 8
        && lower.bytes().any(|b| b != b'/' && b != b':')
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
