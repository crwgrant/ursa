use std::path::PathBuf;

use gpui::{
    Bounds, Font, FontFallbacks, FontFeatures, FontStyle, FontWeight, Hsla, Pixels, Point, SharedString, TextRun, UnderlineStyle,
    Window, fill, point, px, rgb, size,
};
use libghostty_vt::{
    Terminal,
    error::Error,
    render::{CellIterator, RenderState, RowIterator},
    style::RgbColor,
    terminal::{Point as TermPoint, PointCoordinate},
};

use crate::theme;

#[derive(Clone, Debug)]
pub struct Frame {
    pub background: Rgb,
    pub _foreground: Rgb,
    pub rows: Vec<FrameRow>,
    pub cursor: Option<CursorCell>,
    pub has_selection: bool,
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

/// A clickable link and the viewport cells it occupies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkHit {
    pub action: LinkAction,
    pub ranges: Vec<LinkRange>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkAction {
    OpenUrl(String),
    OpenFolder(PathBuf),
}

impl LinkAction {
    pub fn clipboard_text(&self) -> String {
        match self {
            Self::OpenUrl(url) => url.clone(),
            Self::OpenFolder(path) => path.to_string_lossy().into_owned(),
        }
    }

    pub fn open(&self, cx: &mut gpui::App) {
        match self {
            Self::OpenUrl(url) => cx.open_url(url),
            Self::OpenFolder(path) => cx.open_with_system(path),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkRange {
    pub row: u32,
    pub start_col: u16,
    pub end_col: u16,
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
    let mut has_selection = false;

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
                has_selection = true;
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
                Some(span) if span.fg == next.fg && span.bg == next.bg && span.bold == next.bold && span.link == next.link => {
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
        snapshot.cursor_viewport()?.map(|vp| CursorCell { x: vp.x, y: vp.y })
    } else {
        None
    };

    Ok(Frame {
        background: Rgb::from_ghostty(colors.background),
        _foreground: Rgb::from_ghostty(colors.foreground),
        rows,
        cursor,
        has_selection,
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
    pub fn link_at(&self, row: u32, col: u16) -> Option<LinkHit> {
        let row = row as usize;
        if let Some(uri) = hyperlink_at_cell(self, row, col) {
            if let Some(action) = action_from_osc8(&uri) {
                return osc8_hit(self, row, col, &uri, action);
            }
        }
        regex_hit(self, row, col)
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

fn paragraph_at(frame: &Frame, row: usize, col: u16) -> Option<(String, Vec<(usize, u16)>, usize)> {
    if row >= frame.rows.len() {
        return None;
    }
    let (start, end) = wrapped_range(&frame.rows, row);
    let mut text = String::new();
    let mut cells = Vec::new();
    let mut click_at = None;
    for r in start..=end {
        let mut x = 0u16;
        for span in &frame.rows[r].spans {
            if span.text.is_empty() {
                if r == row && col >= x && col < x.saturating_add(span.columns) && click_at.is_none() {
                    click_at = Some(text.len().saturating_sub(1));
                }
                x = x.saturating_add(span.columns);
                continue;
            }
            let mut local = 0u16;
            for ch in span.text.chars() {
                let cell_col = x.saturating_add(local);
                if r == row && col == cell_col && click_at.is_none() {
                    click_at = Some(text.len());
                }
                text.push(ch);
                cells.push((r, cell_col));
                local = local.saturating_add(1);
            }
            if r == row && click_at.is_none() && col >= x && col < x.saturating_add(span.columns) {
                click_at = Some(text.len().saturating_sub(1));
            }
            x = x.saturating_add(span.columns);
        }
    }
    let at = click_at.filter(|_| !text.is_empty())?;
    let at = at.min(text.len().saturating_sub(1));
    Some((text, cells, at))
}

fn osc8_hit(frame: &Frame, row: usize, col: u16, uri: &str, action: LinkAction) -> Option<LinkHit> {
    let (start, end) = wrapped_range(&frame.rows, row);
    let mut cells = Vec::new();
    let mut found = false;
    let mut in_run = false;
    for r in start..=end {
        let mut x = 0u16;
        for span in &frame.rows[r].spans {
            let matches = span.link.as_deref() == Some(uri);
            for _ in 0..span.columns {
                if matches {
                    if r == row && x == col {
                        found = true;
                    }
                    in_run = true;
                    cells.push((r, x));
                } else if in_run {
                    if found {
                        return Some(LinkHit {
                            action,
                            ranges: coalesce_ranges(&cells),
                        });
                    }
                    cells.clear();
                    in_run = false;
                }
                x = x.saturating_add(1);
            }
        }
    }
    if found && !cells.is_empty() {
        Some(LinkHit {
            action,
            ranges: coalesce_ranges(&cells),
        })
    } else {
        None
    }
}

fn regex_hit(frame: &Frame, row: usize, col: u16) -> Option<LinkHit> {
    let (text, cells, at) = paragraph_at(frame, row, col)?;
    if let Some((start, end, url)) = find_web_url_at(&text, at) {
        return hit_from_span(&text, &cells, start, end, LinkAction::OpenUrl(url));
    }
    if let Some((start, end, folder)) = find_path_at(&text, at) {
        return hit_from_span(&text, &cells, start, end, LinkAction::OpenFolder(folder));
    }
    None
}

fn hit_from_span(text: &str, cells: &[(usize, u16)], start: usize, end: usize, action: LinkAction) -> Option<LinkHit> {
    let start_char = text[..start].chars().count();
    let end_char = text[..end].chars().count();
    if start_char >= cells.len() {
        return None;
    }
    let covered = &cells[start_char..end_char.min(cells.len())];
    if covered.is_empty() {
        return None;
    }
    Some(LinkHit {
        action,
        ranges: coalesce_ranges(covered),
    })
}

fn coalesce_ranges(cells: &[(usize, u16)]) -> Vec<LinkRange> {
    let mut ranges: Vec<LinkRange> = Vec::new();
    for &(row, col) in cells {
        match ranges.last_mut() {
            Some(range) if range.row == row as u32 && range.end_col == col => {
                range.end_col = col.saturating_add(1);
            }
            _ => ranges.push(LinkRange {
                row: row as u32,
                start_col: col,
                end_col: col.saturating_add(1),
            }),
        }
    }
    ranges
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

fn find_web_url_at(text: &str, at: usize) -> Option<(usize, usize, String)> {
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
                if is_web_url(&url) {
                    return Some((start, end, url));
                }
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

fn action_from_osc8(uri: &str) -> Option<LinkAction> {
    if is_web_url(uri) {
        return Some(LinkAction::OpenUrl(uri.to_string()));
    }
    folder_from_raw(uri).map(LinkAction::OpenFolder)
}

fn find_path_at(text: &str, at: usize) -> Option<(usize, usize, PathBuf)> {
    if text.is_empty() {
        return None;
    }
    let mut at = at.min(text.len().saturating_sub(1));
    while at > 0 && !text.is_char_boundary(at) {
        at -= 1;
    }
    if !text[at..].chars().next().is_some_and(is_path_char) {
        return None;
    }

    let mut start = at;
    for (idx, ch) in text[..at].char_indices().rev() {
        if is_path_char(ch) {
            start = idx;
        } else {
            break;
        }
    }

    let mut end = at;
    for (rel, ch) in text[at..].char_indices() {
        if is_path_char(ch) {
            end = at + rel + ch.len_utf8();
        } else {
            break;
        }
    }

    let span = &text[start..end];
    let trimmed = trim_path_span(span);
    if trimmed.is_empty() {
        return None;
    }
    let end = start + trimmed.len();
    let folder = folder_from_raw(trimmed)?;
    Some((start, end, folder))
}

fn is_path_char(ch: char) -> bool {
    if ch.is_ascii() {
        matches!(
            ch as u8,
            b'A'..=b'Z'
                | b'a'..=b'z'
                | b'0'..=b'9'
                | b'/'
                | b'.'
                | b'_'
                | b'-'
                | b'~'
                | b'+'
                | b'@'
                | b'%'
                | b':'
        )
    } else {
        !ch.is_whitespace() && !matches!(ch, '<' | '>' | '|' | '"' | '\'')
    }
}

fn trim_path_span(span: &str) -> &str {
    let mut trimmed = span;
    loop {
        if trimmed.len() > 1 {
            let last = trimmed.as_bytes()[trimmed.len() - 1];
            if matches!(last, b'.' | b',' | b';' | b'!' | b'?' | b')' | b']' | b'\'' | b'"') {
                trimmed = &trimmed[..trimmed.len() - 1];
                continue;
            }
        }
        if let Some((head, tail)) = trimmed.rsplit_once(':') {
            if !head.is_empty()
                && !tail.is_empty()
                && tail.bytes().all(|b| b.is_ascii_digit())
                && !head.eq_ignore_ascii_case("file")
            {
                trimmed = head;
                continue;
            }
        }
        break;
    }
    trimmed
}

fn looks_like_path(raw: &str) -> bool {
    if raw == "~" || raw.starts_with("~/") || raw.starts_with("file:") {
        return true;
    }
    if raw.starts_with('/') {
        return raw.len() > 1;
    }
    if is_windows_drive_path(raw) {
        return true;
    }
    let has_separator = raw.contains('/') || raw.contains('\\');
    (raw.starts_with("./") || raw.starts_with(".\\") || raw.starts_with("../") || raw.starts_with("..\\") || has_separator)
        && !raw.starts_with("http:")
        && !raw.starts_with("https:")
}

fn is_windows_drive_path(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && (bytes[2] == b'\\' || bytes[2] == b'/')
}

fn folder_from_raw(raw: &str) -> Option<PathBuf> {
    if !looks_like_path(raw) {
        return None;
    }
    let path = expand_user_path(raw)?;
    if path.is_dir() {
        return Some(path);
    }
    if path.is_file() {
        return path.parent().filter(|parent| parent.is_dir()).map(PathBuf::from);
    }
    None
}

fn expand_user_path(raw: &str) -> Option<PathBuf> {
    if raw.starts_with("file:") {
        return crate::cwd::parse_pwd(raw).filter(|cwd| !cwd.is_remote()).map(|cwd| cwd.path);
    }
    if raw == "~" {
        return crate::pty::home_dir();
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        let mut home = crate::pty::home_dir()?;
        home.push(rest);
        return Some(home);
    }
    if raw.starts_with('/') || is_windows_drive_path(raw) {
        return Some(PathBuf::from(raw));
    }
    std::env::current_dir().ok().map(|cwd| cwd.join(raw))
}

pub fn paint(
    frame: &Frame,
    bounds: Bounds<Pixels>,
    cell: Point<Pixels>,
    font: &Font,
    font_size: Pixels,
    hovered: Option<&LinkHit>,
    focused: bool,
    window: &mut Window,
    cx: &mut gpui::App,
) {
    window.paint_quad(fill(bounds, frame.background.to_hsla()));

    let colors = theme::colors(cx);
    let mut wash: Hsla = rgb(colors.accent).into();
    wash.a = 0.22;
    let link_color: Hsla = rgb(colors.accent).into();
    let cursor_color = rgb(colors.cursor);
    let underline = UnderlineStyle {
        thickness: px(1.0),
        color: Some(link_color),
        wavy: false,
    };

    let mut y = bounds.origin.y + theme::TERMINAL_PAD;
    for (row_idx, row) in frame.rows.iter().enumerate() {
        let highlights = row_highlights(hovered, row_idx);

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

        for &(start_col, end_col) in &highlights {
            let width = cell.x * (end_col.saturating_sub(start_col)) as f32;
            if width > px(0.0) {
                window.paint_quad(fill(
                    Bounds {
                        origin: point(bounds.origin.x + theme::TERMINAL_PAD + cell.x * start_col as f32, y),
                        size: size(width, cell.y),
                    },
                    wash,
                ));
            }
        }

        // A block cursor is painted under glyphs so the current cell stays
        // readable and lines up with the same grid used for text.
        if focused && crate::config::cursor_shape(cx) == crate::config::CursorShape::Block {
            if let Some(cursor) = frame.cursor.filter(|cursor| cursor.y as usize == row_idx) {
                window.paint_quad(fill(
                    Bounds {
                        origin: point(bounds.origin.x + theme::TERMINAL_PAD + cell.x * cursor.x as f32, y),
                        size: size(cell.x, cell.y),
                    },
                    cursor_color,
                ));
            }
        }

        x = bounds.origin.x + theme::TERMINAL_PAD;
        let mut col = 0u16;
        for span in &row.spans {
            paint_span_text(span, col, x, y, cell, font, font_size, &highlights, link_color, underline, window, cx);
            x += cell.x * span.columns as f32;
            col = col.saturating_add(span.columns);
        }

        // A bar cursor sits on top of the glyph so it stays visible.
        if focused && crate::config::cursor_shape(cx) == crate::config::CursorShape::Bar {
            if let Some(cursor) = frame.cursor.filter(|cursor| cursor.y as usize == row_idx) {
                let width = (cell.x * 0.12).max(px(1.5));
                window.paint_quad(fill(
                    Bounds {
                        origin: point(bounds.origin.x + theme::TERMINAL_PAD + cell.x * cursor.x as f32, y),
                        size: size(width, cell.y),
                    },
                    cursor_color,
                ));
            }
        }

        y += cell.y;
    }
}

fn row_highlights(hovered: Option<&LinkHit>, row: usize) -> Vec<(u16, u16)> {
    let Some(hit) = hovered else {
        return Vec::new();
    };
    hit.ranges
        .iter()
        .filter(|range| range.row as usize == row)
        .map(|range| (range.start_col, range.end_col))
        .collect()
}

fn paint_span_text(
    span: &FrameSpan,
    span_col: u16,
    x: Pixels,
    y: Pixels,
    cell: Point<Pixels>,
    font: &Font,
    font_size: Pixels,
    highlights: &[(u16, u16)],
    link_color: Hsla,
    underline: UnderlineStyle,
    window: &mut Window,
    cx: &mut gpui::App,
) {
    let span_end = span_col.saturating_add(span.columns);
    let mut cursor = span_col;
    let mut origin_x = x;
    let mut cuts: Vec<(u16, u16, bool)> = Vec::new();
    for &(h0, h1) in highlights {
        let start = h0.max(span_col);
        let end = h1.min(span_end);
        if start >= end {
            continue;
        }
        if cursor < start {
            cuts.push((cursor, start, false));
        }
        cuts.push((start, end, true));
        cursor = end;
    }
    if cursor < span_end {
        cuts.push((cursor, span_end, false));
    }
    if cuts.is_empty() {
        cuts.push((span_col, span_end, false));
    }

    for (from, to, highlighted) in cuts {
        let columns = to.saturating_sub(from);
        let width = cell.x * columns as f32;
        let relative = from.saturating_sub(span_col);
        let text: String = span.text.chars().skip(relative as usize).take(columns as usize).collect();
        if !text.is_empty() {
            let run_font = if span.bold || highlighted {
                font.clone().bold()
            } else {
                font.clone()
            };
            let line = window.text_system().shape_line(
                SharedString::from(text.clone()),
                font_size,
                &[TextRun {
                    len: text.len(),
                    font: run_font,
                    color: if highlighted { link_color } else { span.fg.to_hsla() },
                    background_color: None,
                    underline: highlighted.then_some(underline),
                    strikethrough: None,
                }],
                Some(cell.x),
            );
            let _ = line.paint(point(origin_x, y), cell.y, window, cx);
        }
        origin_x += width;
    }
}

pub fn terminal_font(cx: &gpui::App) -> Font {
    let family = crate::config::font_family(cx);
    Font {
        family: family.clone(),
        features: terminal_font_features(),
        fallbacks: terminal_font_fallbacks(family.as_ref()),
        weight: FontWeight::NORMAL,
        style: FontStyle::Normal,
    }
}

/// DirectWrite turns `liga` / `clig` / `calt` on unless they are explicitly
/// disabled. Fonts such as JetBrains Mono then fold pairs like `!=` into one
/// glyph, and GPUI's per-glyph `force_width` snap no longer matches the
/// character grid.
fn terminal_font_features() -> FontFeatures {
    FontFeatures(std::sync::Arc::new(vec![("calt".into(), 0), ("liga".into(), 0), ("clig".into(), 0)]))
}

fn terminal_font_fallbacks(primary: &str) -> Option<FontFallbacks> {
    let mut fonts = Vec::new();
    let mut push = |name: &str| {
        if name.eq_ignore_ascii_case(primary) {
            return;
        }
        if fonts.iter().any(|existing: &String| existing.eq_ignore_ascii_case(name)) {
            return;
        }
        fonts.push(name.to_string());
    };
    for name in crate::config::default_font_candidates() {
        push(name);
    }
    #[cfg(target_os = "macos")]
    {
        push("Menlo");
        push("Apple Color Emoji");
    }
    #[cfg(target_os = "windows")]
    {
        push("Cascadia Mono");
        push("Consolas");
        push("Courier New");
        push("Segoe UI Emoji");
    }
    if fonts.is_empty() {
        None
    } else {
        Some(FontFallbacks::from_fonts(fonts))
    }
}

pub fn measure_cell(window: &mut Window, cx: &gpui::App) -> Point<Pixels> {
    let font = terminal_font(cx);
    let font_size = px(crate::config::font_size(cx));
    // Average a run of identical glyphs so rounding in the shaper does not
    // inflate a single "M" into a too-wide cell.
    const SAMPLE: &str = "MMMMMMMM";
    let line = window.text_system().shape_line(
        SharedString::from(SAMPLE),
        font_size,
        &[TextRun {
            len: SAMPLE.len(),
            font,
            color: rgb(theme::colors(cx).text).into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }],
        None,
    );
    let width = (line.width / SAMPLE.len() as f32).max(px(1.0));
    point(width, px(crate::config::font_size(cx) * theme::LINE_HEIGHT))
}
