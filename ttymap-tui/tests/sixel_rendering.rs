use ttymap_engine::geo::LonLat;
use ttymap_engine::map::render::frame::{MapCell, MapFrame};
use ttymap_tui::terminal_graphics::{
    ActiveRenderMode, CellPixelSize, RenderMode, frame_text_overlays, frame_to_sixel,
    resolve_render_mode,
};

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
fn sixel_frame_encoding_is_stable_for_public_api() {
    let sixel = frame_to_sixel(&sample_frame(), CellPixelSize::new(4, 8));
    assert!(sixel.starts_with("\x1bPq\"1;1;8;8"));
    assert!(sixel.contains("#4;2;0;0;50"));
    assert!(sixel.contains("#9;2;100;0;0"));
    assert!(sixel.ends_with("\x1b\\"));
}

#[test]
fn text_overlay_extraction_only_returns_non_braille_cells() {
    let overlays = frame_text_overlays(&sample_frame());
    assert_eq!(overlays.len(), 1);
    assert_eq!(overlays[0].x, 1);
    assert_eq!(overlays[0].symbol, "A");
}

#[test]
fn explicit_sixel_mode_bypasses_auto_detection() {
    assert_eq!(
        resolve_render_mode(RenderMode::Sixel),
        ActiveRenderMode::Sixel
    );
}
