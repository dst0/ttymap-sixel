use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::io::{self, IsTerminal, Write};

use clap::ValueEnum;
use crossterm::cursor::{MoveTo, RestorePosition, SavePosition};
use crossterm::queue;
use crossterm::style::{
    Color as CrosstermColor, Print, ResetColor, SetBackgroundColor, SetForegroundColor,
};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color as RatatuiColor;
use ttymap_engine::map::render::frame::MapFrame;

const BRAILLE_BITS: [[u8; 2]; 4] = [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]];
const DEFAULT_CELL_PIXEL_WIDTH: u16 = 8;
const DEFAULT_CELL_PIXEL_HEIGHT: u16 = 16;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum RenderMode {
    #[default]
    Auto,
    Braille,
    Sixel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActiveRenderMode {
    Braille,
    Sixel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CellPixelSize {
    pub width: u16,
    pub height: u16,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum SnapshotFormat {
    #[default]
    Ansi,
    Sixel,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OverlayCell {
    pub x: u16,
    pub y: u16,
    pub symbol: String,
    pub fg: CrosstermColor,
    pub bg: CrosstermColor,
}

impl CellPixelSize {
    pub const fn new(width: u16, height: u16) -> Self {
        Self { width, height }
    }

    fn normalized(self) -> Self {
        Self {
            width: self.width.max(1),
            height: self.height.max(1),
        }
    }
}

impl Default for ActiveRenderMode {
    fn default() -> Self {
        Self::Braille
    }
}

impl Default for CellPixelSize {
    fn default() -> Self {
        Self::new(DEFAULT_CELL_PIXEL_WIDTH, DEFAULT_CELL_PIXEL_HEIGHT)
    }
}

pub fn resolve_render_mode(mode: RenderMode) -> ActiveRenderMode {
    match mode {
        RenderMode::Auto => {
            if terminal_supports_sixel() {
                ActiveRenderMode::Sixel
            } else {
                ActiveRenderMode::Braille
            }
        }
        RenderMode::Braille => ActiveRenderMode::Braille,
        RenderMode::Sixel => ActiveRenderMode::Sixel,
    }
}

pub fn terminal_supports_sixel() -> bool {
    io::stdout().is_terminal()
        && terminal_supports_sixel_env(
            std::env::var("TERM").ok().as_deref(),
            std::env::var("TERM_PROGRAM").ok().as_deref(),
            std::env::var("XTERM_VERSION").ok().as_deref(),
        )
}

pub fn terminal_supports_sixel_env(
    term: Option<&str>,
    term_program: Option<&str>,
    _xterm_version: Option<&str>,
) -> bool {
    let term = term.unwrap_or_default().to_ascii_lowercase();
    let term_program = term_program.unwrap_or_default().to_ascii_lowercase();

    term.contains("sixel")
        || matches!(
            term.as_str(),
            "mlterm" | "mintty" | "contour" | "foot" | "yaft"
        )
        || term.starts_with("wezterm")
        || matches!(
            term_program.as_str(),
            "wezterm" | "iterm.app" | "mintty" | "mlterm" | "contour" | "foot"
        )
}

pub fn terminal_cell_pixel_size() -> CellPixelSize {
    let fallback = CellPixelSize::default();
    let Ok(size) = crossterm::terminal::window_size() else {
        return fallback;
    };
    if size.columns == 0 || size.rows == 0 || size.width == 0 || size.height == 0 {
        return fallback;
    }
    CellPixelSize::new(
        (size.width / size.columns).max(1),
        (size.height / size.rows).max(1),
    )
}

pub fn frame_to_sixel(frame: &MapFrame, cell_size: CellPixelSize) -> String {
    if frame.cols == 0 || frame.rows == 0 || frame.cells.is_empty() {
        return String::new();
    }

    let cell_size = cell_size.normalized();
    let cols = frame.cols as usize;
    let rows = frame.rows as usize;
    let width = cols * cell_size.width as usize;
    let height = rows * cell_size.height as usize;
    let mut pixels = vec![0u8; width * height];

    for row in 0..rows {
        for col in 0..cols {
            let Some(cell) = frame.cells.get(row * cols + col) else {
                continue;
            };
            let cell_x = col * cell_size.width as usize;
            let cell_y = row * cell_size.height as usize;
            fill_rect(
                &mut pixels,
                width,
                cell_x,
                cell_y,
                cell_size.width as usize,
                cell_size.height as usize,
                cell.bg,
            );
            if let Some(bits) = braille_bits(cell.ch) {
                for (dot_y, row_bits) in BRAILLE_BITS.iter().enumerate() {
                    for (dot_x, bit) in row_bits.iter().enumerate() {
                        if bits & *bit == 0 {
                            continue;
                        }
                        let x0 = cell_x + dot_x * cell_size.width as usize / 2;
                        let x1 = cell_x + (dot_x + 1) * cell_size.width as usize / 2;
                        let y0 = cell_y + dot_y * cell_size.height as usize / 4;
                        let y1 = cell_y + (dot_y + 1) * cell_size.height as usize / 4;
                        fill_rect(&mut pixels, width, x0, y0, x1 - x0, y1 - y0, cell.fg);
                    }
                }
            }
        }
    }

    encode_sixel(&pixels, width, height)
}

pub fn frame_text_overlays(frame: &MapFrame) -> Vec<OverlayCell> {
    let cols = frame.cols as usize;
    frame
        .cells
        .iter()
        .enumerate()
        .filter_map(|(idx, cell)| {
            if cell.ch.is_whitespace() || braille_bits(cell.ch).is_some() {
                return None;
            }
            Some(OverlayCell {
                x: (idx % cols) as u16,
                y: (idx / cols) as u16,
                symbol: cell.ch.to_string(),
                fg: CrosstermColor::AnsiValue(cell.fg),
                bg: CrosstermColor::AnsiValue(cell.bg),
            })
        })
        .collect()
}

pub fn buffer_overlays(buffer: &Buffer, area: Rect) -> Vec<OverlayCell> {
    let mut overlays = Vec::new();
    for y in 0..area.height {
        for x in 0..area.width {
            let cell = &buffer[(area.x + x, area.y + y)];
            if cell.symbol() == " " {
                continue;
            }
            overlays.push(OverlayCell {
                x,
                y,
                symbol: cell.symbol().to_string(),
                fg: ratatui_to_crossterm(cell.fg),
                bg: ratatui_to_crossterm(cell.bg),
            });
        }
    }
    overlays
}

pub fn paint_sixel_frame(
    writer: &mut impl Write,
    frame: &MapFrame,
    area: Rect,
    cell_size: CellPixelSize,
    overlays: &[OverlayCell],
) -> io::Result<()> {
    if area.width == 0 || area.height == 0 || frame.cols == 0 || frame.rows == 0 {
        return Ok(());
    }

    queue!(writer, SavePosition, MoveTo(area.x, area.y))?;
    write!(writer, "{}", frame_to_sixel(frame, cell_size))?;

    for overlay in frame_text_overlays(frame)
        .into_iter()
        .chain(overlays.iter().cloned())
    {
        queue!(
            writer,
            MoveTo(area.x + overlay.x, area.y + overlay.y),
            SetForegroundColor(overlay.fg),
            SetBackgroundColor(overlay.bg),
            Print(overlay.symbol),
        )?;
    }

    queue!(writer, ResetColor, RestorePosition)?;
    writer.flush()
}

fn encode_sixel(pixels: &[u8], width: usize, height: usize) -> String {
    let used_colors: BTreeSet<u8> = pixels.iter().copied().collect();
    let mut out = String::new();
    out.push_str("\x1bPq");
    let _ = write!(out, "\"1;1;{};{}", width, height);
    for color in &used_colors {
        let (r, g, b) = xterm_to_rgb(*color);
        let _ = write!(
            out,
            "#{};2;{};{};{}",
            color,
            rgb_to_percent(r),
            rgb_to_percent(g),
            rgb_to_percent(b)
        );
    }

    let groups = height.div_ceil(6);
    for group in 0..groups {
        let mut color_lines = Vec::new();
        for color in &used_colors {
            let mut line = String::new();
            let _ = write!(line, "#{}", color);
            let mut chars = Vec::with_capacity(width);
            let mut any = false;
            for x in 0..width {
                let mut bits = 0u8;
                for bit in 0..6 {
                    let y = group * 6 + bit;
                    if y < height && pixels[y * width + x] == *color {
                        bits |= 1 << bit;
                        any = true;
                    }
                }
                chars.push((63 + bits) as char);
            }
            if !any {
                continue;
            }
            push_rle(&mut line, &chars);
            color_lines.push(line);
        }

        if !color_lines.is_empty() {
            out.push_str(&color_lines.join("$"));
        }
        if group + 1 < groups {
            out.push('-');
        }
    }

    out.push_str("\x1b\\");
    out
}

fn push_rle(out: &mut String, chars: &[char]) {
    let mut iter = chars.iter().copied();
    let Some(mut current) = iter.next() else {
        return;
    };
    let mut count = 1usize;
    for ch in iter {
        if ch == current {
            count += 1;
            continue;
        }
        push_run(out, current, count);
        current = ch;
        count = 1;
    }
    push_run(out, current, count);
}

fn push_run(out: &mut String, ch: char, count: usize) {
    if count >= 4 {
        let _ = write!(out, "!{}{ch}", count);
        return;
    }
    for _ in 0..count {
        out.push(ch);
    }
}

fn fill_rect(
    pixels: &mut [u8],
    width: usize,
    x: usize,
    y: usize,
    rect_width: usize,
    rect_height: usize,
    color: u8,
) {
    for py in y..y + rect_height {
        let row = py * width;
        for px in x..x + rect_width {
            pixels[row + px] = color;
        }
    }
}

fn braille_bits(ch: char) -> Option<u8> {
    let code = ch as u32;
    if (0x2800..=0x28ff).contains(&code) {
        Some((code - 0x2800) as u8)
    } else {
        None
    }
}

fn rgb_to_percent(v: u8) -> u8 {
    ((u16::from(v) * 100) / 255) as u8
}

fn ratatui_to_crossterm(color: RatatuiColor) -> CrosstermColor {
    match color {
        RatatuiColor::Reset => CrosstermColor::Reset,
        RatatuiColor::Black => CrosstermColor::Black,
        RatatuiColor::Red => CrosstermColor::DarkRed,
        RatatuiColor::Green => CrosstermColor::DarkGreen,
        RatatuiColor::Yellow => CrosstermColor::DarkYellow,
        RatatuiColor::Blue => CrosstermColor::DarkBlue,
        RatatuiColor::Magenta => CrosstermColor::DarkMagenta,
        RatatuiColor::Cyan => CrosstermColor::DarkCyan,
        RatatuiColor::Gray => CrosstermColor::Grey,
        RatatuiColor::DarkGray => CrosstermColor::DarkGrey,
        RatatuiColor::LightRed => CrosstermColor::Red,
        RatatuiColor::LightGreen => CrosstermColor::Green,
        RatatuiColor::LightYellow => CrosstermColor::Yellow,
        RatatuiColor::LightBlue => CrosstermColor::Blue,
        RatatuiColor::LightMagenta => CrosstermColor::Magenta,
        RatatuiColor::LightCyan => CrosstermColor::Cyan,
        RatatuiColor::White => CrosstermColor::White,
        RatatuiColor::Rgb(r, g, b) => CrosstermColor::Rgb { r, g, b },
        RatatuiColor::Indexed(idx) => CrosstermColor::AnsiValue(idx),
    }
}

fn xterm_to_rgb(idx: u8) -> (u8, u8, u8) {
    match idx {
        0 => (0x00, 0x00, 0x00),
        1 => (0x80, 0x00, 0x00),
        2 => (0x00, 0x80, 0x00),
        3 => (0x80, 0x80, 0x00),
        4 => (0x00, 0x00, 0x80),
        5 => (0x80, 0x00, 0x80),
        6 => (0x00, 0x80, 0x80),
        7 => (0xc0, 0xc0, 0xc0),
        8 => (0x80, 0x80, 0x80),
        9 => (0xff, 0x00, 0x00),
        10 => (0x00, 0xff, 0x00),
        11 => (0xff, 0xff, 0x00),
        12 => (0x00, 0x00, 0xff),
        13 => (0xff, 0x00, 0xff),
        14 => (0x00, 0xff, 0xff),
        15 => (0xff, 0xff, 0xff),
        16..=231 => {
            let idx = idx - 16;
            let r = idx / 36;
            let g = (idx % 36) / 6;
            let b = idx % 6;
            (cube_value(r), cube_value(g), cube_value(b))
        }
        232..=255 => {
            let shade = 8 + (idx - 232) * 10;
            (shade, shade, shade)
        }
    }
}

fn cube_value(component: u8) -> u8 {
    if component == 0 {
        0
    } else {
        55 + component * 40
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ttymap_engine::geo::LonLat;
    use ttymap_engine::map::render::frame::MapCell;

    fn sample_frame() -> MapFrame {
        MapFrame {
            cells: vec![
                MapCell {
                    ch: '\u{2801}',
                    fg: 9,
                    bg: 0,
                },
                MapCell {
                    ch: 'A',
                    fg: 15,
                    bg: 4,
                },
            ],
            cols: 2,
            rows: 1,
            center: LonLat { lon: 0.0, lat: 0.0 },
            zoom: 0.0,
        }
    }

    #[test]
    fn auto_mode_detects_known_sixel_terminals() {
        assert!(terminal_supports_sixel_env(Some("mlterm"), None, None));
        assert!(terminal_supports_sixel_env(None, Some("WezTerm"), None));
        assert!(!terminal_supports_sixel_env(
            Some("xterm-256color"),
            None,
            None
        ));
    }

    #[test]
    fn frame_to_sixel_emits_dcs_sequence_and_palette() {
        let sixel = frame_to_sixel(&sample_frame(), CellPixelSize::new(4, 8));
        assert!(sixel.starts_with("\x1bPq\"1;1;8;8"));
        assert!(sixel.contains("#0;2;0;0;0"));
        assert!(sixel.contains("#9;2;100;0;0"));
        assert!(sixel.ends_with("\x1b\\"));
    }

    #[test]
    fn frame_text_overlays_skip_braille_cells() {
        let overlays = frame_text_overlays(&sample_frame());
        assert_eq!(overlays.len(), 1);
        assert_eq!(overlays[0].x, 1);
        assert_eq!(overlays[0].symbol, "A");
    }

    #[test]
    fn buffer_overlays_only_collect_visible_symbols() {
        let area = Rect::new(0, 0, 2, 1);
        let mut buf = Buffer::empty(area);
        buf[(0, 0)].set_char('X');
        let overlays = buffer_overlays(&buf, area);
        assert_eq!(overlays.len(), 1);
        assert_eq!(overlays[0].symbol, "X");
    }
}
