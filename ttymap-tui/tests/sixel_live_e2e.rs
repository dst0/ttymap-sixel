use std::io::IsTerminal;

use ratatui::layout::Rect;
use ttymap_engine::geo::LonLat;
use ttymap_engine::map::render::frame::{MapCell, MapFrame};
use ttymap_tui::terminal_graphics::{CellPixelSize, OverlayCell, paint_sixel_frame};

#[test]
#[ignore = "requires a real terminal with sixel support"]
fn paints_sixel_frame_to_real_terminal() {
    if !std::io::stdout().is_terminal() {
        return;
    }

    let frame = MapFrame {
        cells: vec![
            MapCell {
                ch: '\u{2801}',
                fg: 9,
                bg: 0,
            },
            MapCell {
                ch: 'L',
                fg: 15,
                bg: 4,
            },
        ],
        cols: 2,
        rows: 1,
        center: LonLat { lon: 0.0, lat: 0.0 },
        zoom: 0.0,
    };

    let overlays = [OverlayCell {
        x: 1,
        y: 0,
        symbol: "E".to_string(),
        fg: crossterm::style::Color::White,
        bg: crossterm::style::Color::AnsiValue(4),
    }];

    paint_sixel_frame(
        &mut std::io::stdout(),
        &frame,
        Rect::new(0, 0, 2, 1),
        CellPixelSize::new(4, 8),
        &overlays,
    )
    .expect("paint sixel frame");
}
