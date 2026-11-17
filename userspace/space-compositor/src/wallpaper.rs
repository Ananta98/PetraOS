//! Procedural Petra Slate desktop wallpaper (ReactOS-inspired deep slate-cobalt gradient
//! with subtle geometric facets and corner system watermark).

use space_core::font;
use space_core::icons::{self, IconId};
use space_core::{Color, PixelBuffer};

#[inline]
fn mix_rgb(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// Builds a `w x h` ARGB8888 Petra Slate wallpaper buffer.
pub fn build_petra_wallpaper(w: u32, h: u32) -> Vec<u32> {
    let wi = w.max(1) as usize;
    let hi = h.max(1) as usize;
    let mut pb = PixelBuffer::new(wi as u32, hi as u32, Color::rgb(0x22, 0x4C, 0x7E));

    let wf = wi as f32;
    let hf = hi as f32;

    let top_col = [42.0, 90.0, 146.0];
    let mid_col = [31.0, 71.0, 122.0];
    let bot_col = [20.0, 48.0, 86.0];

    for y in 0..hi {
        let v = y as f32 / hf;
        let row_base = if v < 0.55 {
            mix_rgb(top_col, mid_col, v / 0.55)
        } else {
            mix_rgb(mid_col, bot_col, (v - 0.55) / 0.45)
        };

        for x in 0..wi {
            let u = x as f32 / wf;
            // Subtle radial illumination centered around the upper-right third
            let dx = u - 0.62;
            let dy = v - 0.36;
            let glow = (-(dx * dx * 3.2 + dy * dy * 4.5)).exp() * 18.0;

            // Very restrained crystalline facet planes for PetraOS character
            let facet1 = if (u * 0.85 + v * 0.55) > 0.82 { 4.5 } else { 0.0 };
            let facet2 = if (u * 0.45 - v * 0.72) > 0.05 { 3.0 } else { 0.0 };

            let r = (row_base[0] + glow * 0.65 + facet1 + facet2).clamp(0.0, 255.0) as u8;
            let g = (row_base[1] + glow * 0.85 + facet1 + facet2).clamp(0.0, 255.0) as u8;
            let b = (row_base[2] + glow * 1.15 + facet1 * 1.3 + facet2).clamp(0.0, 255.0) as u8;
            pb.data[y * wi + x] = Color::rgb(r, g, b).0;
        }
    }

    // Subtle crisp PetraOS emblem & build watermark above bottom-right taskbar
    if wi >= 320 && hi >= 200 {
        let mark_x = wi as i32 - 224;
        let mark_y = hi as i32 - 66;
        icons::draw_icon(&mut pb, mark_x, mark_y - 2, 24, IconId::PetraLogo);
        font::draw_text_bold(&mut pb, mark_x + 30, mark_y - 1, "PetraOS", Color::argb(190, 226, 238, 252), 2);
        font::draw_text(
            &mut pb,
            mark_x + 30,
            mark_y + 15,
            "PetraSpace  •  Petra Slate",
            Color::argb(165, 178, 206, 238),
            1,
        );
    }

    pb.data
}
