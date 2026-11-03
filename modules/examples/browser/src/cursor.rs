//! Mouse cursors: vector shapes rasterised once (anti-aliased, vello_cpu) into
//! a small atlas texture that the frame reducer draws at the pointer.

use anyrender::PaintScene;
use cursor_icon::CursorIcon;
use kurbo::{Affine, BezPath, Stroke};
use peniko::{Color, Fill};

/// Side of one atlas cell, in pixels.
pub const CELL: u32 = 32;
const SHAPES: u32 = 4;

/// Atlas size (one row of cells).
pub fn atlas_size() -> (u32, u32) {
    (CELL * SHAPES, CELL)
}

/// Atlas cell and hotspot (pixels inside the cell) for an icon.
pub fn cell(icon: CursorIcon) -> (u32, (f32, f32)) {
    use CursorIcon as C;
    match icon {
        C::Pointer | C::Grab | C::Grabbing => (1, (9.4, 2.4)),
        C::Text | C::VerticalText => (2, (5.0, 11.0)),
        C::NotAllowed | C::NoDrop => (3, (11.0, 11.0)),
        _ => (0, (2.0, 2.0)),
    }
}

/// Straight-alpha RGBA pixels of the atlas.
pub fn atlas() -> Vec<u8> {
    let (w, h) = atlas_size();
    let mut p = anyrender_vello_cpu::VelloCpuScenePainter::new(w as u16, h as u16);
    let black = Color::from_rgb8(0, 0, 0);
    let white = Color::WHITE;
    let path = |d: &str| BezPath::from_svg(d).expect("cursor path");
    let at = |cell: u32| Affine::translate(((cell * CELL) as f64, 0.0));

    // 0: arrow — black body, white rim.
    let arrow = path("M2 2 L2 19.5 L6.6 15.4 L9.6 22.2 L12.6 20.9 L9.7 14.3 L15.8 14.3 Z");
    p.stroke(&Stroke::new(2.4).with_join(kurbo::Join::Round), at(0), white, None, &arrow);
    p.fill(Fill::NonZero, at(0), black, None, &arrow);

    // 1: pointing hand — white glove, black outline and finger creases.
    let hand = path(
        "M6 14 L6 3.5 C6 1.4 9 1.4 9 3.5 L9 9.6 \
         C9 8.1 11.7 8.1 11.7 9.6 L11.7 10.4 \
         C11.7 8.9 14.4 8.9 14.4 10.4 L14.4 11.2 \
         C14.4 9.8 17 9.8 17 11.2 L17 16.5 \
         C17 19.6 15.6 21.6 14.2 23 L8.2 23 \
         C6.8 21.6 4.8 19.2 3.2 16.6 L1.9 14.3 \
         C1.2 13 2.7 11.6 4 12.6 Z",
    );
    let creases = path("M9 10 L9 13.4 M11.7 10.6 L11.7 13.4 M14.4 11.4 L14.4 13.4");
    let big = at(1) * Affine::scale(1.25);
    p.fill(Fill::NonZero, big, white, None, &hand);
    p.stroke(&Stroke::new(0.85).with_join(kurbo::Join::Round), big, black, None, &hand);
    p.stroke(&Stroke::new(0.7).with_caps(kurbo::Cap::Round), big, black, None, &creases);

    // 2: I-beam — black with a white halo so it shows on dark text too.
    let beam = path(
        "M2.5 2.5 C4 2.5 5 3 5 4.5 C5 3 6 2.5 7.5 2.5 M5 4.5 L5 17.5 \
         M2.5 19.5 C4 19.5 5 19 5 17.5 C5 19 6 19.5 7.5 19.5",
    );
    p.stroke(&Stroke::new(3.2).with_caps(kurbo::Cap::Round), at(2), white, None, &beam);
    p.stroke(&Stroke::new(1.3).with_caps(kurbo::Cap::Round), at(2), black, None, &beam);

    // 3: not-allowed — circle with a slash.
    let ring = kurbo::Circle::new((11.0, 11.0), 7.0);
    let slash = kurbo::Line::new((6.0, 16.0), (16.0, 6.0));
    for (width, color) in [(4.6, white), (2.2, black)] {
        p.stroke(&Stroke::new(width), at(3), color, None, &ring);
        p.stroke(&Stroke::new(width), at(3), color, None, &slash);
    }

    let mut px = p.finish().data_as_u8_slice().to_vec();
    // vello_cpu produces premultiplied alpha; the compositor blends straight.
    for c in px.chunks_exact_mut(4) {
        let a = c[3] as u32;
        if a != 0 && a != 255 {
            for v in &mut c[..3] {
                *v = ((*v as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    px
}
