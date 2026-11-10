//! The visible screen as paintable rows: resolved colors, merged style runs, grid sizing.

use alacritty_terminal::Term;
use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor, Rgb};

use super::palette::{DARK, LIGHT, ThemePalette, rgb};

pub(crate) const MIN_COLS: usize = 20;
pub(crate) const MIN_ROWS: usize = 8;
/// Share of the text color kept by faint (SGR 2) text.
const DIM: f32 = 0.66;
pub(crate) const SEARCH_MATCH_BG: Rgb = Rgb { r: 94, g: 80, b: 36 };
pub(crate) const ACTIVE_SEARCH_MATCH_BG: Rgb = Rgb { r: 166, g: 128, b: 38 };

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SearchMatch {
    pub line: i32,
    pub start_col: usize,
    pub end_col: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Palette {
    ansi: [Rgb; 16],
    pub fg: Rgb,
    pub bg: Rgb,
    pub cursor: Rgb,
    pub selection: Rgb,
}

impl Palette {
    #[allow(dead_code)]
    pub fn new(dark: bool, fg: Rgb, bg: Rgb) -> Self {
        Self {
            ansi: if dark { DARK } else { LIGHT },
            fg,
            bg,
            cursor: fg,
            selection: if dark { rgb(0x404040) } else { rgb(0xd4d4d4) },
        }
    }

    pub fn from_theme(palette: ThemePalette) -> Self {
        Self {
            ansi: palette.ansi,
            fg: palette.fg,
            bg: palette.bg,
            cursor: palette.cursor,
            selection: palette.selection,
        }
    }

    /// Color `index` as xterm numbers them; 256 and up are alacritty's named slots.
    pub fn index(&self, index: usize) -> Rgb {
        let level = |v: usize| if v == 0 { 0 } else { 55 + 40 * v as u8 };
        match index {
            0..=15 => self.ansi[index],
            16..=231 => {
                let i = index - 16;
                Rgb { r: level(i / 36), g: level(i / 6 % 6), b: level(i % 6) }
            }
            232..=255 => {
                let v = 8 + 10 * (index - 232) as u8;
                Rgb { r: v, g: v, b: v }
            }
            256 => self.fg,
            257 => self.bg,
            258 => self.cursor,
            259..=266 => mix(self.ansi[index - 259], self.bg, DIM),
            268 => mix(self.fg, self.bg, DIM),
            _ => self.fg,
        }
    }

    pub fn resolve(&self, color: Color, overrides: &Colors) -> Rgb {
        match color {
            Color::Spec(rgb) => rgb,
            Color::Indexed(i) => overrides[i as usize].unwrap_or_else(|| self.index(i as usize)),
            Color::Named(n) => overrides[n].unwrap_or_else(|| self.index(n as usize)),
        }
    }
}

impl From<ThemePalette> for Palette {
    fn from(palette: ThemePalette) -> Self {
        Self::from_theme(palette)
    }
}

/// Text on an explicit background is pushed toward black or white until it reaches this.
const MIN_CONTRAST: f32 = 4.5;

fn luminance(c: Rgb) -> f32 {
    static LINEAR: std::sync::LazyLock<[f32; 256]> = std::sync::LazyLock::new(|| {
        std::array::from_fn(|v| {
            let s = v as f32 / 255.;
            if s <= 0.03928 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
        })
    });
    0.2126 * LINEAR[c.r as usize] + 0.7152 * LINEAR[c.g as usize] + 0.0722 * LINEAR[c.b as usize]
}

/// WCAG contrast ratio.
pub(crate) fn contrast(a: Rgb, b: Rgb) -> f32 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// `fg`, blended toward white or black (away from `bg` first) just enough to
/// reach 4.5:1 on `bg`; the better extreme when neither gets there.
pub(crate) fn readable(fg: Rgb, bg: Rgb) -> Rgb {
    if contrast(fg, bg) >= MIN_CONTRAST {
        return fg;
    }
    let (white, black) = (rgb(0xffffff), rgb(0x000000));
    let toward = |target: Rgb| {
        (1..=20).map(|i| mix(target, fg, i as f32 / 20.)).find(|c| contrast(*c, bg) >= MIN_CONTRAST)
    };
    let (first, second) =
        if luminance(fg) >= luminance(bg) { (white, black) } else { (black, white) };
    toward(first)
        .or_else(|| toward(second))
        .unwrap_or(if contrast(white, bg) >= contrast(black, bg) { white } else { black })
}

/// `t` of `a` blended with the rest from `b`.
pub(crate) fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let m = |x: u8, y: u8| (x as f32 * t + y as f32 * (1. - t)).round() as u8;
    Rgb { r: m(a.r, b.r), g: m(a.g, b.g), b: m(a.b, b.b) }
}

/// A cell's text color and background (`None` for the terminal's own) after
/// inverse, faint and hidden. Program colors stay as sent, as xterm draws them.
pub(crate) fn cell_colors(
    fg: Color,
    bg: Color,
    flags: Flags,
    p: &Palette,
    overrides: &Colors,
) -> (Rgb, Option<Rgb>) {
    let mut fg = p.resolve(fg, overrides);
    let mut bg = (bg != Color::Named(NamedColor::Background)).then(|| p.resolve(bg, overrides));
    if flags.contains(Flags::INVERSE) {
        (fg, bg) = (bg.unwrap_or(p.bg), Some(fg));
    }
    let under = bg.unwrap_or(p.bg);
    if flags.contains(Flags::HIDDEN) {
        fg = under;
    } else if flags.contains(Flags::DIM) {
        fg = mix(fg, under, DIM);
    }
    (fg, bg)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Style {
    pub fg: Rgb,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub undercurl: bool,
    pub strike: bool,
}

impl Style {
    fn decorated(&self) -> bool {
        self.underline || self.undercurl || self.strike
    }
}

#[derive(Debug, PartialEq)]
pub(crate) struct Run {
    /// UTF-8 bytes of the row text this run covers.
    pub len: usize,
    pub style: Style,
}

/// One screen row: one char per column (wide-char spacers as spaces), so glyph
/// `i` belongs to column `i` when shaped at a forced cell width.
#[derive(Debug, Default)]
pub(crate) struct Row {
    pub text: String,
    pub runs: Vec<Run>,
    /// Non-default backgrounds as `(first column, end column, color)`.
    pub bgs: Vec<(usize, usize, Rgb)>,
    pub selected: Option<(usize, usize)>,
    /// Box-drawing and block characters, painted as shapes so lines join across cells.
    pub boxes: Vec<(usize, char, Rgb)>,
}

impl Row {
    pub fn push(&mut self, col: usize, c: char, zerowidth: &[char], style: Style, bg: Option<Rgb>) {
        let start = self.text.len();
        self.text.push(c);
        self.text.extend(zerowidth);
        let len = self.text.len() - start;
        match self.runs.last_mut() {
            Some(run) if run.style == style => run.len += len,
            _ => self.runs.push(Run { len, style }),
        }
        if let Some(bg) = bg {
            match self.bgs.last_mut() {
                Some((_, end, color)) if *end == col && *color == bg => *end += 1,
                _ => self.bgs.push((col, col + 1, bg)),
            }
        }
    }

    pub fn select(&mut self, col: usize) {
        self.selected =
            Some(self.selected.map_or((col, col + 1), |(s, e)| (s.min(col), e.max(col + 1))));
    }

    /// Drops trailing blanks nothing is drawn for; backgrounds are painted separately.
    pub fn trim_end(&mut self) {
        while self.text.ends_with(' ')
            && let Some(run) = self.runs.last_mut()
            && !run.style.decorated()
        {
            self.text.pop();
            run.len -= 1;
            if run.len == 0 {
                self.runs.pop();
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FrameCursor {
    pub row: usize,
    pub col: usize,
    /// 2 on a wide character.
    pub width: usize,
    pub shape: CursorShape,
}

pub(crate) struct Frame {
    pub rows: Vec<Row>,
    pub cursor: Option<FrameCursor>,
    /// The program asked for a blinking cursor (DECSCUSR).
    pub blinking: bool,
}

fn match_highlight_at(
    active_matches: &[(i32, usize, usize, bool)],
    line: i32,
    col: usize,
) -> Option<bool> {
    let mut matched = false;
    for &(m_line, start_col, end_col, is_active) in active_matches {
        if m_line == line && (start_col..end_col).contains(&col) {
            if is_active {
                return Some(true);
            }
            matched = true;
        }
    }
    matched.then_some(false)
}

/// The visible screen as rows. With `block`, the text under a block cursor takes
/// its cell's background so it reads on the cursor.
#[cfg(test)]
pub(crate) fn snapshot<T: EventListener>(term: &Term<T>, p: &Palette, block: bool) -> Frame {
    snapshot_with_matches(term, p, block, &[])
}

/// The visible screen as rows, highlighting `active_matches` (`(line, start_col, end_col, is_active)`).
pub(crate) fn snapshot_with_matches<T: EventListener>(
    term: &Term<T>,
    p: &Palette,
    block: bool,
    active_matches: &[(i32, usize, usize, bool)],
) -> Frame {
    let content = term.renderable_content();
    let offset = content.display_offset as i32;
    let mut rows: Vec<Row> = (0..term.screen_lines()).map(|_| Row::default()).collect();
    let point = content.cursor.point;
    let cursor_row = point.line.0 + offset;
    let mut cursor = (content.cursor.shape != CursorShape::Hidden
        && (0..rows.len() as i32).contains(&cursor_row))
    .then_some(FrameCursor {
        row: cursor_row as usize,
        col: point.column.0,
        width: 1,
        shape: content.cursor.shape,
    });
    for cell in content.display_iter {
        let line = cell.point.line.0;
        let (row, col) = ((line + offset) as usize, cell.point.column.0);
        let flags = cell.flags;
        let (mut fg, mut bg) = cell_colors(cell.fg, cell.bg, flags, p, content.colors);
        if let Some(is_active) = match_highlight_at(active_matches, line, col) {
            let highlight_bg = if is_active { ACTIVE_SEARCH_MATCH_BG } else { SEARCH_MATCH_BG };
            bg = Some(highlight_bg);
            if !flags.contains(Flags::HIDDEN) {
                fg = readable(fg, highlight_bg);
            }
        }
        if let Some(c) = cursor.as_mut()
            && (c.row, c.col) == (row, col)
        {
            if flags.contains(Flags::WIDE_CHAR) {
                c.width = 2;
            }
            if block && c.shape == CursorShape::Block {
                fg = bg.unwrap_or(p.bg);
            }
        }
        let spacer = flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER);
        let (mut ch, zerowidth) = if spacer || cell.c == '\t' {
            (' ', &[][..])
        } else {
            (cell.c, cell.zerowidth().unwrap_or(&[]))
        };
        if super::boxes::is_box(ch) && zerowidth.is_empty() {
            rows[row].boxes.push((col, ch, fg));
            ch = ' ';
        }
        let style = Style {
            fg,
            bold: flags.contains(Flags::BOLD),
            italic: flags.contains(Flags::ITALIC),
            underline: flags.intersects(Flags::ALL_UNDERLINES) && !flags.contains(Flags::UNDERCURL),
            undercurl: flags.contains(Flags::UNDERCURL),
            strike: flags.contains(Flags::STRIKEOUT),
        };
        rows[row].push(col, ch, zerowidth, style, bg);
        if content.selection.is_some_and(|s| s.contains(cell.point)) {
            rows[row].select(col);
        }
    }
    rows.iter_mut().for_each(Row::trim_end);
    Frame { rows, cursor, blinking: term.cursor_style().blinking }
}

/// Convenience wrapper over [`snapshot_with_matches`] for a slice of [`SearchMatch`] and optional active index.
pub(crate) fn snapshot_search_matches<T: EventListener>(
    term: &Term<T>,
    p: &Palette,
    block: bool,
    matches: &[SearchMatch],
    active_index: Option<usize>,
) -> Frame {
    if matches.is_empty() {
        return snapshot_with_matches(term, p, block, &[]);
    }
    let active_matches: Vec<(i32, usize, usize, bool)> = matches
        .iter()
        .enumerate()
        .map(|(idx, m)| (m.line, m.start_col, m.end_col, active_index == Some(idx)))
        .collect();
    snapshot_with_matches(term, p, block, &active_matches)
}

/// Case-insensitive substring search across `(line, text)` pairs.
/// Returns empty when `query.trim()` is empty.
pub(crate) fn search_lines<'a>(
    lines: impl IntoIterator<Item = (i32, &'a str)>,
    query: &str,
) -> Vec<SearchMatch> {
    if query.trim().is_empty() {
        return Vec::new();
    }
    let needle: Vec<char> = query.chars().map(|c| c.to_lowercase().next().unwrap_or(c)).collect();
    if needle.is_empty() {
        return Vec::new();
    }
    const MAX_MATCHES: usize = 1000;
    let mut matches = Vec::new();
    for (line, text) in lines {
        let hay: Vec<char> = text.chars().map(|c| c.to_lowercase().next().unwrap_or(c)).collect();
        let mut col = 0;
        while col + needle.len() <= hay.len() {
            if hay[col..col + needle.len()] == needle[..] {
                matches.push(SearchMatch { line, start_col: col, end_col: col + needle.len() });
                if matches.len() >= MAX_MATCHES {
                    return matches;
                }
                col += needle.len();
            } else {
                col += 1;
            }
        }
    }
    matches
}

/// Case-insensitive search across all scrollback history and visible screen lines of `term`.
pub(crate) fn search_term<T: EventListener>(term: &Term<T>, query: &str) -> Vec<SearchMatch> {
    if query.trim().is_empty() {
        return Vec::new();
    }
    let cols = term.columns();
    let lines: Vec<(i32, String)> = (term.topmost_line().0..=term.bottommost_line().0)
        .map(|line_idx| {
            let row = &term.grid()[Line(line_idx)];
            let mut text = String::with_capacity(cols);
            for col in 0..cols {
                let cell = &row[Column(col)];
                let spacer = cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER);
                text.push(if spacer || cell.c == '\t' || cell.c == '\0' { ' ' } else { cell.c });
            }
            (line_idx, text)
        })
        .collect();
    search_lines(lines.iter().map(|(l, s)| (*l, s.as_str())), query)
}

/// Computes the `display_offset` that keeps `line` visible in the viewport,
/// preserving `current_offset` if `line` is already visible and otherwise centering
/// `line` within `screen_lines` clamped to `0..=history_size`.
pub(crate) fn display_offset_for_line(
    line: i32,
    screen_lines: usize,
    history_size: usize,
    current_offset: usize,
) -> usize {
    if screen_lines == 0 || history_size == 0 {
        return 0;
    }
    let current = current_offset.min(history_size);
    let row = line + current as i32;
    if (0..screen_lines as i32).contains(&row) {
        return current;
    }
    if line >= 0 {
        return 0;
    }
    let half = (screen_lines / 2) as i32;
    let desired = (-line).saturating_add(half) as usize;
    desired.min(history_size)
}

/// Columns and rows of `cell_w`×`cell_h` cells that fit `width`×`height`, at least 20×8.
pub(crate) fn grid_size(width: f32, height: f32, cell_w: f32, cell_h: f32) -> (usize, usize) {
    let fit = |len: f32, cell: f32, min: usize| {
        if cell > 0. { ((len / cell).floor() as usize).max(min) } else { min }
    };
    (fit(width, cell_w, MIN_COLS), fit(height, cell_h, MIN_ROWS))
}

/// First row to show when only `visible` of `rows` fit, keeping the cursor in view.
pub(crate) fn first_visible_row(rows: usize, visible: usize, cursor_row: Option<usize>) -> usize {
    let hidden = rows.saturating_sub(visible);
    cursor_row.map_or(hidden, |c| (c + 1).saturating_sub(visible)).min(hidden)
}

#[cfg(test)]
#[path = "../tests/unit/grid.rs"]
mod tests;
