//! Crisp 16x16 and 24x24 ARGB pixel-art/procedural icon system with alpha blending.

use crate::geometry::Rect;
use crate::pixel::{Canvas, Color};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconId {
    StartFlag,
    Files,
    Terminal,
    Tasks,
    Notepad,
    Calc,
    ControlPanel,
    MyComputer,
}

impl IconId {
    /// Infers an icon from a window title or binary name.
    pub fn from_title(title: &str) -> Self {
        let lower = title.to_ascii_lowercase();
        if lower.contains("file") {
            Self::Files
        } else if lower.contains("term") {
            Self::Terminal
        } else if lower.contains("task") {
            Self::Tasks
        } else if lower.contains("pad") || lower.contains("note") {
            Self::Notepad
        } else if lower.contains("calc") {
            Self::Calc
        } else if lower.contains("control") || lower.contains("setting") {
            Self::ControlPanel
        } else {
            Self::MyComputer
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowGlyph {
    Minimize,
    Maximize,
    Restore,
    Close,
}

struct IconBuf<'a> {
    size: i32,
    px: &'a mut [u32],
}

impl<'a> IconBuf<'a> {
    #[inline]
    fn set(&mut self, x: i32, y: i32, c: Color) {
        if x >= 0 && x < self.size && y >= 0 && y < self.size {
            self.px[(y * self.size + x) as usize] = c.0;
        }
    }

    fn fill(&mut self, x: i32, y: i32, w: i32, h: i32, c: Color) {
        for py in y..(y + h) {
            for px in x..(x + w) {
                self.set(px, py, c);
            }
        }
    }

    fn outline(&mut self, x: i32, y: i32, w: i32, h: i32, c: Color) {
        self.fill(x, y, w, 1, c);
        self.fill(x, y + h - 1, w, 1, c);
        self.fill(x, y, 1, h, c);
        self.fill(x + w - 1, y, 1, h, c);
    }

    fn blit_16_template(&mut self, rows: &[&str; 16], palette: &[(char, Color)]) {
        for (y, row) in rows.iter().enumerate() {
            for (x, ch) in row.chars().take(16).enumerate() {
                if let Some(&(_, col)) = palette.iter().find(|&&(k, _)| k == ch) {
                    self.set(x as i32, y as i32, col);
                }
            }
        }
    }
}

fn paint_16(b: &mut IconBuf<'_>, id: IconId) {
    match id {
        IconId::StartFlag => {
            // Classic 4-color waving flag
            let rows = [
                "................",
                "...rrrr.........",
                "..rrrrrr..gggg..",
                "..rrrrrr.gggggg.",
                "..rrrrrr.gggggg.",
                "..rrrrrr.gggggg.",
                "...rrrr..gggggg.",
                "..........gggg..",
                "...bbbb.........",
                "..bbbbbb..yyyy..",
                "..bbbbbb.yyyyyy.",
                "..bbbbbb.yyyyyy.",
                "..bbbbbb.yyyyyy.",
                "...bbbb..yyyyyy.",
                "..........yyyy..",
                "................",
            ];
            let pal = [
                ('r', Color::rgb(235, 76, 38)),
                ('g', Color::rgb(118, 185, 36)),
                ('b', Color::rgb(42, 132, 232)),
                ('y', Color::rgb(248, 194, 34)),
            ];
            b.blit_16_template(&rows, &pal);
            // Subtle highlight pixels on each pane
            b.set(3, 2, Color::rgb(255, 160, 120));
            b.set(11, 3, Color::rgb(190, 235, 110));
            b.set(3, 9, Color::rgb(130, 195, 255));
            b.set(11, 10, Color::rgb(255, 235, 140));
        }
        IconId::Files => {
            // Yellow XP folder
            let out = Color::rgb(168, 112, 24);
            let tab = Color::rgb(245, 200, 72);
            let body = Color::rgb(252, 218, 104);
            let hi = Color::rgb(255, 242, 170);
            let sh = Color::rgb(224, 168, 48);
            b.fill(2, 3, 5, 2, tab);
            b.fill(1, 5, 14, 9, body);
            b.outline(1, 5, 14, 9, out);
            b.fill(2, 2, 5, 1, out);
            b.set(1, 3, out);
            b.set(1, 4, out);
            b.set(7, 3, out);
            b.set(7, 4, out);
            b.fill(2, 6, 12, 1, hi);
            b.fill(2, 12, 12, 1, sh);
            b.fill(2, 14, 12, 1, Color::argb(90, 0, 0, 0));
        }
        IconId::Terminal => {
            // Dark console with >_ prompt
            b.fill(1, 2, 14, 12, Color::rgb(24, 28, 34));
            b.outline(1, 2, 14, 12, Color::rgb(96, 104, 116));
            b.fill(2, 3, 12, 2, Color::rgb(0, 84, 227));
            let fg = Color::rgb(88, 240, 108);
            // '>'
            b.set(3, 7, fg);
            b.set(4, 8, fg);
            b.set(5, 9, fg);
            b.set(4, 10, fg);
            b.set(3, 11, fg);
            // '_'
            b.fill(7, 11, 4, 1, fg);
        }
        IconId::Tasks => {
            // System monitor graph
            b.fill(1, 2, 14, 12, Color::rgb(14, 34, 20));
            b.outline(1, 2, 14, 12, Color::rgb(112, 118, 128));
            let grid = Color::rgb(24, 68, 36);
            b.fill(2, 6, 12, 1, grid);
            b.fill(2, 10, 12, 1, grid);
            b.fill(6, 3, 1, 10, grid);
            b.fill(10, 3, 1, 10, grid);
            let wave = Color::rgb(64, 244, 96);
            for (x, y) in [
                (2, 10), (3, 10), (4, 9), (5, 7), (6, 4), (7, 9),
                (8, 11), (9, 8), (10, 6), (11, 9), (12, 9), (13, 8),
            ] {
                b.set(x, y, wave);
            }
        }
        IconId::Notepad => {
            // Lined page + pen
            b.fill(2, 1, 10, 14, Color::rgb(252, 252, 246));
            b.outline(2, 1, 10, 14, Color::rgb(120, 132, 148));
            b.fill(3, 2, 8, 2, Color::rgb(52, 120, 216));
            let line_c = Color::rgb(168, 194, 226);
            b.fill(4, 6, 6, 1, line_c);
            b.fill(4, 9, 6, 1, line_c);
            b.fill(4, 12, 5, 1, line_c);
            // Yellow/orange pen on the right
            for i in 0..7 {
                b.set(13 - i / 2, 5 + i, Color::rgb(240, 176, 32));
                b.set(14 - i / 2, 5 + i, Color::rgb(184, 116, 16));
            }
            b.set(9, 12, Color::rgb(64, 64, 64));
        }
        IconId::Calc => {
            // Calculator body + display + grid
            b.fill(2, 1, 12, 14, Color::rgb(212, 216, 224));
            b.outline(2, 1, 12, 14, Color::rgb(84, 96, 112));
            b.fill(4, 3, 8, 3, Color::rgb(188, 224, 192));
            b.outline(4, 3, 8, 3, Color::rgb(96, 120, 100));
            for gy in 0..3 {
                for gx in 0..3 {
                    let col = if gx == 2 && gy == 2 {
                        Color::rgb(220, 84, 48)
                    } else {
                        Color::rgb(68, 116, 196)
                    };
                    b.fill(4 + gx * 3, 7 + gy * 2, 2, 1, col);
                }
            }
        }
        IconId::ControlPanel => {
            // Gear / sliders
            let metal = Color::rgb(132, 146, 164);
            let dark = Color::rgb(68, 80, 96);
            let hi = Color::rgb(224, 232, 240);
            b.fill(5, 4, 6, 8, metal);
            b.fill(4, 5, 8, 6, metal);
            // Teeth
            b.fill(7, 2, 2, 2, dark);
            b.fill(7, 12, 2, 2, dark);
            b.fill(2, 7, 2, 2, dark);
            b.fill(12, 7, 2, 2, dark);
            b.fill(3, 3, 2, 2, metal);
            b.fill(11, 3, 2, 2, metal);
            b.fill(3, 11, 2, 2, metal);
            b.fill(11, 11, 2, 2, metal);
            b.fill(6, 6, 4, 4, hi);
            b.fill(7, 7, 2, 2, Color::rgb(48, 116, 216));
        }
        IconId::MyComputer => {
            // CRT/LCD monitor + base
            b.fill(1, 2, 14, 10, Color::rgb(220, 224, 230));
            b.outline(1, 2, 14, 10, Color::rgb(92, 104, 120));
            b.fill(3, 4, 10, 6, Color::rgb(42, 126, 224));
            b.fill(3, 4, 10, 2, Color::rgb(104, 178, 248));
            b.fill(6, 12, 4, 2, Color::rgb(156, 164, 176));
            b.fill(4, 14, 8, 1, Color::rgb(92, 104, 120));
        }
    }
}

fn paint_24(b: &mut IconBuf<'_>, id: IconId) {
    // Crisp 24x24 icons with richer detail & drop shadows
    match id {
        IconId::StartFlag => {
            let r = Color::rgb(235, 76, 38);
            let g = Color::rgb(118, 185, 36);
            let bl = Color::rgb(42, 132, 232);
            let y = Color::rgb(248, 194, 34);
            b.fill(3, 3, 8, 8, r);
            b.fill(13, 4, 8, 8, g);
            b.fill(3, 13, 8, 8, bl);
            b.fill(13, 14, 8, 8, y);
            // Gloss highlights
            b.fill(4, 4, 5, 2, Color::rgb(255, 152, 112));
            b.fill(14, 5, 5, 2, Color::rgb(184, 232, 104));
            b.fill(4, 14, 5, 2, Color::rgb(126, 192, 255));
            b.fill(14, 15, 5, 2, Color::rgb(255, 232, 132));
        }
        IconId::Files => {
            let out = Color::rgb(168, 112, 24);
            let tab = Color::rgb(245, 200, 72);
            let body = Color::rgb(252, 218, 104);
            let hi = Color::rgb(255, 244, 180);
            let sh = Color::rgb(220, 162, 42);
            b.fill(2, 4, 8, 3, tab);
            b.outline(2, 4, 8, 3, out);
            b.fill(2, 7, 20, 14, body);
            b.outline(2, 7, 20, 14, out);
            b.fill(3, 8, 18, 2, hi);
            b.fill(3, 18, 18, 2, sh);
            b.fill(3, 21, 18, 1, Color::argb(90, 0, 0, 0));
        }
        IconId::Terminal => {
            b.fill(2, 3, 20, 18, Color::rgb(22, 26, 32));
            b.outline(2, 3, 20, 18, Color::rgb(96, 108, 124));
            b.fill(3, 4, 18, 3, Color::rgb(0, 84, 227));
            let fg = Color::rgb(88, 244, 112);
            for i in 0..4 {
                b.fill(5 + i, 10 + i, 2, 1, fg);
                b.fill(5 + i, 16 - i, 2, 1, fg);
            }
            b.fill(11, 16, 6, 2, fg);
        }
        IconId::Tasks => {
            b.fill(2, 3, 20, 18, Color::rgb(14, 34, 20));
            b.outline(2, 3, 20, 18, Color::rgb(104, 112, 124));
            let grid = Color::rgb(24, 68, 36);
            b.fill(3, 9, 18, 1, grid);
            b.fill(3, 15, 18, 1, grid);
            b.fill(9, 4, 1, 16, grid);
            b.fill(15, 4, 1, 16, grid);
            let wave = Color::rgb(64, 248, 96);
            for (x, y) in [
                (3, 15), (4, 15), (5, 14), (6, 11), (7, 7), (8, 5),
                (9, 10), (10, 16), (11, 17), (12, 13), (13, 9), (14, 12),
                (15, 14), (16, 14), (17, 11), (18, 8), (19, 13), (20, 14),
            ] {
                b.fill(x, y, 1, 2, wave);
            }
        }
        IconId::Notepad => {
            b.fill(3, 2, 15, 20, Color::rgb(252, 252, 246));
            b.outline(3, 2, 15, 20, Color::rgb(112, 124, 140));
            b.fill(4, 3, 13, 3, Color::rgb(52, 120, 216));
            let line_c = Color::rgb(168, 194, 226);
            b.fill(5, 9, 10, 1, line_c);
            b.fill(5, 13, 10, 1, line_c);
            b.fill(5, 17, 8, 1, line_c);
            for i in 0..10 {
                b.fill(20 - i / 2, 7 + i, 2, 1, Color::rgb(242, 178, 34));
            }
            b.fill(14, 17, 2, 2, Color::rgb(64, 64, 64));
        }
        IconId::Calc => {
            b.fill(3, 2, 18, 20, Color::rgb(214, 218, 226));
            b.outline(3, 2, 18, 20, Color::rgb(84, 96, 112));
            b.fill(5, 4, 14, 5, Color::rgb(188, 224, 192));
            b.outline(5, 4, 14, 5, Color::rgb(96, 120, 100));
            for gy in 0..3 {
                for gx in 0..3 {
                    let col = if gx == 2 && gy == 2 {
                        Color::rgb(220, 84, 48)
                    } else {
                        Color::rgb(68, 116, 196)
                    };
                    b.fill(5 + gx * 5, 11 + gy * 3, 3, 2, col);
                }
            }
        }
        IconId::ControlPanel => {
            let metal = Color::rgb(138, 152, 170);
            let dark = Color::rgb(68, 80, 96);
            let hi = Color::rgb(232, 238, 246);
            b.fill(7, 6, 10, 12, metal);
            b.fill(6, 7, 12, 10, metal);
            b.fill(10, 3, 4, 3, dark);
            b.fill(10, 18, 4, 3, dark);
            b.fill(3, 10, 3, 4, dark);
            b.fill(18, 10, 3, 4, dark);
            b.fill(5, 5, 3, 3, metal);
            b.fill(16, 5, 3, 3, metal);
            b.fill(5, 16, 3, 3, metal);
            b.fill(16, 16, 3, 3, metal);
            b.fill(9, 9, 6, 6, hi);
            b.fill(10, 10, 4, 4, Color::rgb(48, 116, 216));
        }
        IconId::MyComputer => {
            b.fill(2, 3, 20, 14, Color::rgb(224, 228, 236));
            b.outline(2, 3, 20, 14, Color::rgb(88, 100, 116));
            b.fill(4, 5, 16, 9, Color::rgb(42, 126, 224));
            b.fill(4, 5, 16, 3, Color::rgb(104, 178, 248));
            b.fill(9, 17, 6, 3, Color::rgb(156, 164, 176));
            b.fill(6, 20, 12, 2, Color::rgb(92, 104, 120));
        }
    }
}

/// Draws a 16x16 or 24x24 ARGB icon with alpha blending, clipped to `canvas.bounds()`.
pub fn draw_icon(canvas: &mut dyn Canvas, x: i32, y: i32, size: i32, id: IconId) {
    if size <= 16 {
        let mut buf = [0u32; 16 * 16];
        let mut ib = IconBuf { size: 16, px: &mut buf };
        paint_16(&mut ib, id);
        let clip = canvas.bounds();
        canvas.blit_from(&buf, 16, Rect::new(0, 0, 16, 16), x, y, clip, true);
    } else {
        let mut buf = [0u32; 24 * 24];
        let mut ib = IconBuf { size: 24, px: &mut buf };
        paint_24(&mut ib, id);
        let clip = canvas.bounds();
        canvas.blit_from(&buf, 24, Rect::new(0, 0, 24, 24), x, y, clip, true);
    }
}

/// Draws crisp window control glyphs (`_`, `□`, `×`) centered inside `rect`, clipped to `canvas.bounds()`.
pub fn draw_window_glyph(canvas: &mut dyn Canvas, rect: Rect, glyph: WindowGlyph, color: Color) {
    let cx = rect.x + rect.w / 2;
    let cy = rect.y + rect.h / 2;
    match glyph {
        WindowGlyph::Minimize => {
            canvas.fill_rect(Rect::new(cx - 4, cy + 2, 8, 2), color);
        }
        WindowGlyph::Maximize => {
            canvas.fill_rect(Rect::new(cx - 4, cy - 4, 9, 2), color);
            canvas.draw_rect_outline(Rect::new(cx - 4, cy - 4, 9, 9), 1, color);
        }
        WindowGlyph::Restore => {
            canvas.draw_rect_outline(Rect::new(cx - 2, cy - 5, 7, 7), 1, color);
            canvas.draw_rect_outline(Rect::new(cx - 5, cy - 2, 7, 7), 1, color);
        }
        WindowGlyph::Close => {
            for i in -4..=4i32 {
                canvas.fill_rect(Rect::new(cx + i, cy + i, 2, 2), color);
                canvas.fill_rect(Rect::new(cx + i, cy - i, 2, 2), color);
            }
        }
    }
}
