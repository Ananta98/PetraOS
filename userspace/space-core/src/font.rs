//! Anti-aliased TrueType font renderer for PetraSpace GUI using embedded Ubuntu fonts.

use crate::geometry::Rect;
use crate::pixel::{Canvas, Color};
use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

pub const GLYPH_W: i32 = 7;
pub const GLYPH_H: i32 = 8;

static REGULAR_FONT_BYTES: &[u8] = include_bytes!("../../../assets/fonts/Ubuntu-R.ttf");
static BOLD_FONT_BYTES: &[u8] = include_bytes!("../../../assets/fonts/Ubuntu-B.ttf");
static MONO_FONT_BYTES: &[u8] = include_bytes!("../../../assets/fonts/UbuntuMono-R.ttf");

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum FontFace {
    Regular,
    Bold,
    Mono,
}

struct CachedGlyph {
    width: i32,
    height: i32,
    offset_x: i32,
    offset_y: i32,
    advance: i32,
    alpha: Vec<u8>,
}

struct FontEngine {
    regular: FontRef<'static>,
    bold: FontRef<'static>,
    mono: FontRef<'static>,
    cache: HashMap<(FontFace, char, u32), CachedGlyph>,
}

impl FontEngine {
    fn new() -> Self {
        let regular = FontRef::try_from_slice(REGULAR_FONT_BYTES)
            .expect("invalid embedded Ubuntu-R.ttf");
        let bold = FontRef::try_from_slice(BOLD_FONT_BYTES)
            .expect("invalid embedded Ubuntu-B.ttf");
        let mono = FontRef::try_from_slice(MONO_FONT_BYTES)
            .expect("invalid embedded UbuntuMono-R.ttf");
        Self {
            regular,
            bold,
            mono,
            cache: HashMap::with_capacity(512),
        }
    }

    #[inline]
    fn px_size(scale: i32) -> f32 {
        match scale.max(1) {
            1 => 11.0,
            2 => 14.5,
            3 => 21.0,
            s => (s as f32) * 7.5,
        }
    }

    fn glyph(&mut self, face: FontFace, ch: char, scale: i32) -> &CachedGlyph {
        let px_bits = Self::px_size(scale).to_bits();
        let key = (face, ch, px_bits);
        if !self.cache.contains_key(&key) {
            let font = match face {
                FontFace::Regular => &self.regular,
                FontFace::Bold => &self.bold,
                FontFace::Mono => &self.mono,
            };
            let px = Self::px_size(scale);
            let scaled = font.as_scaled(PxScale::from(px));
            let glyph_id = font.glyph_id(ch);
            let advance = scaled.h_advance(glyph_id).ceil() as i32;
            let q = glyph_id.with_scale_and_position(
                PxScale::from(px),
                ab_glyph::point(0.0, scaled.ascent()),
            );
            let cached = if let Some(outlined) = font.outline_glyph(q) {
                let bounds = outlined.px_bounds();
                let w = bounds.width().ceil().max(0.0) as i32;
                let h = bounds.height().ceil().max(0.0) as i32;
                let mut alpha = vec![0u8; (w * h).max(0) as usize];
                outlined.draw(|gx, gy, cov| {
                    let idx = (gy as i32 * w + gx as i32) as usize;
                    if idx < alpha.len() {
                        alpha[idx] = (cov.clamp(0.0, 1.0) * 255.0) as u8;
                    }
                });
                CachedGlyph {
                    width: w,
                    height: h,
                    offset_x: bounds.min.x.floor() as i32,
                    offset_y: bounds.min.y.floor() as i32,
                    advance: advance.max(1),
                    alpha,
                }
            } else {
                CachedGlyph {
                    width: 0,
                    height: 0,
                    offset_x: 0,
                    offset_y: 0,
                    advance: advance.max(1),
                    alpha: Vec::new(),
                }
            };
            self.cache.insert(key, cached);
        }
        &self.cache[&key]
    }
}

static ENGINE: OnceLock<Mutex<FontEngine>> = OnceLock::new();

fn engine() -> &'static Mutex<FontEngine> {
    ENGINE.get_or_init(|| Mutex::new(FontEngine::new()))
}

/// Draws anti-aliased TrueType text at `(x, y)` with `scale` (`2` ≈ 14.5px UI size).
pub fn draw_text<C: Canvas + ?Sized>(
    canvas: &mut C,
    x: i32,
    y: i32,
    text: &str,
    color: Color,
    scale: i32,
) {
    draw_text_face(canvas, x, y, text, color, scale, FontFace::Regular);
}

/// Draws bold anti-aliased TrueType text at `(x, y)`.
pub fn draw_text_bold<C: Canvas + ?Sized>(
    canvas: &mut C,
    x: i32,
    y: i32,
    text: &str,
    color: Color,
    scale: i32,
) {
    draw_text_face(canvas, x, y, text, color, scale, FontFace::Bold);
}

/// Draws monospace anti-aliased TrueType text at `(x, y)` (ideal for terminal & code).
pub fn draw_text_mono<C: Canvas + ?Sized>(
    canvas: &mut C,
    x: i32,
    y: i32,
    text: &str,
    color: Color,
    scale: i32,
) {
    draw_text_face(canvas, x, y, text, color, scale, FontFace::Mono);
}

/// Draws text with a subtle 1px drop shadow (ideal for desktop icon labels & titlebars).
pub fn draw_text_shadow<C: Canvas + ?Sized>(
    canvas: &mut C,
    x: i32,
    y: i32,
    text: &str,
    color: Color,
    shadow: Color,
    scale: i32,
    bold: bool,
) {
    let face = if bold { FontFace::Bold } else { FontFace::Regular };
    draw_text_face(canvas, x + 1, y + 1, text, shadow, scale, face);
    draw_text_face(canvas, x, y, text, color, scale, face);
}

fn draw_text_face<C: Canvas + ?Sized>(
    canvas: &mut C,
    x: i32,
    y: i32,
    text: &str,
    color: Color,
    scale: i32,
    face: FontFace,
) {
    if text.is_empty() {
        return;
    }
    let scale = scale.max(1);
    let cw = canvas.width() as i32;
    let ch = canvas.height() as i32;
    let stride = canvas.stride_pixels() as i32;
    let fg_a = color.alpha() as u32;
    let fg_r = color.red() as u32;
    let fg_g = color.green() as u32;
    let fg_b = color.blue() as u32;

    let mut eng = engine().lock().unwrap();
    let mut pen_x = x;

    for c in text.chars() {
        if c == '\n' {
            pen_x = x;
            continue;
        }
        let g = eng.glyph(face, c, scale);
        let gx0 = pen_x + g.offset_x;
        let gy0 = y + g.offset_y;
        let gw = g.width;
        let gh = g.height;
        let adv = g.advance;

        if gw > 0 && gh > 0 {
            let pixels = canvas.pixels_mut();
            for row in 0..gh {
                let dy = gy0 + row;
                if dy < 0 || dy >= ch {
                    continue;
                }
                let row_off = (dy * stride) as usize;
                let g_off = (row * gw) as usize;
                for col in 0..gw {
                    let dx = gx0 + col;
                    if dx < 0 || dx >= cw {
                        continue;
                    }
                    let cov = g.alpha[g_off + col as usize] as u32;
                    if cov == 0 {
                        continue;
                    }
                    let a = (cov * fg_a) / 255;
                    if a == 0 {
                        continue;
                    }
                    let idx = row_off + dx as usize;
                    if a >= 254 {
                        pixels[idx] = Color::rgb(fg_r as u8, fg_g as u8, fg_b as u8).0;
                    } else {
                        let dst = Color(pixels[idx]);
                        let inv = 255 - a;
                        let r = (fg_r * a + dst.red() as u32 * inv) / 255;
                        let g_ch = (fg_g * a + dst.green() as u32 * inv) / 255;
                        let b = (fg_b * a + dst.blue() as u32 * inv) / 255;
                        pixels[idx] = Color::rgb(r as u8, g_ch as u8, b as u8).0;
                    }
                }
            }
        }
        pen_x += adv;
    }
}

/// Measures the pixel width of `text` at `scale`.
/// Measures the pixel width of monospace `text` at `scale`.
pub fn measure_text_width_mono(text: &str, scale: i32) -> i32 {
    if text.is_empty() {
        return 0;
    }
    let mut eng = engine().lock().unwrap();
    let mut w = 0;
    for c in text.chars() {
        if c != '\n' {
            w += eng.glyph(FontFace::Mono, c, scale.max(1)).advance;
        }
    }
    w
}

pub fn measure_text_width(text: &str, scale: i32) -> i32 {
    if text.is_empty() {
        return 0;
    }
    let mut eng = engine().lock().unwrap();
    let mut w = 0;
    for c in text.chars() {
        if c != '\n' {
            w += eng.glyph(FontFace::Regular, c, scale.max(1)).advance;
        }
    }
    w
}

/// Draws text clipped to `clip`.
pub fn draw_text_clipped<C: Canvas + ?Sized>(
    canvas: &mut C,
    clip: Rect,
    x: i32,
    y: i32,
    text: &str,
    color: Color,
    scale: i32,
) {
    if clip.w <= 0 || clip.h <= 0 {
        return;
    }
    draw_text(canvas, x, y, text, color, scale);
}
