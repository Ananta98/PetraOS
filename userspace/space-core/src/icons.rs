//! Smooth anti-aliased ARGB icon renderer (32x32 master + bilinear scaling) for PetraSpace GUI.

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

struct MasterCanvas {
    size: i32,
    px: Vec<u32>,
}

impl MasterCanvas {
    fn new(size: i32) -> Self {
        Self {
            size,
            px: vec![0u32; (size * size) as usize],
        }
    }

    #[inline]
    fn blend(&mut self, x: i32, y: i32, c: Color) {
        if x < 0 || y < 0 || x >= self.size || y >= self.size {
            return;
        }
        let a = c.alpha() as u32;
        if a == 0 {
            return;
        }
        let idx = (y * self.size + x) as usize;
        if a >= 254 {
            self.px[idx] = c.0;
            return;
        }
        let dst = Color(self.px[idx]);
        let da = dst.alpha() as u32;
        if da == 0 {
            self.px[idx] = c.0;
            return;
        }
        let inv = 255 - a;
        let out_a = (a + (da * inv) / 255).min(255);
        let r = (c.red() as u32 * a + dst.red() as u32 * inv) / 255;
        let g = (c.green() as u32 * a + dst.green() as u32 * inv) / 255;
        let b = (c.blue() as u32 * a + dst.blue() as u32 * inv) / 255;
        self.px[idx] = Color::argb(out_a as u8, r as u8, g as u8, b as u8).0;
    }

    fn fill_rounded_grad(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        top: Color,
        bot: Color,
    ) {
        let x0 = (x.floor() as i32 - 1).max(0);
        let y0 = (y.floor() as i32 - 1).max(0);
        let x1 = ((x + w).ceil() as i32 + 1).min(self.size);
        let y1 = ((y + h).ceil() as i32 + 1).min(self.size);
        let cx = x + w * 0.5;
        let cy = y + h * 0.5;
        let hx = w * 0.5 - r;
        let hy = h * 0.5 - r;

        for py in y0..y1 {
            let fy = py as f32 + 0.5;
            let t = ((fy - y) / h.max(1.0)).clamp(0.0, 1.0);
            let col = Color::lerp(top, bot, (t * 255.0) as i32, 255);
            for px in x0..x1 {
                let fx = px as f32 + 0.5;
                let dx = (fx - cx).abs() - hx;
                let dy = (fy - cy).abs() - hy;
                let dist = dx.max(0.0).hypot(dy.max(0.0)) + dx.max(dy).min(0.0) - r;
                let cov = (0.5 - dist).clamp(0.0, 1.0);
                if cov > 0.0 {
                    let a = ((col.alpha() as f32) * cov) as u8;
                    self.blend(px, py, col.with_alpha(a));
                }
            }
        }
    }

    fn fill_circle(&mut self, cx: f32, cy: f32, r: f32, col: Color) {
        let x0 = ((cx - r).floor() as i32 - 1).max(0);
        let y0 = ((cy - r).floor() as i32 - 1).max(0);
        let x1 = ((cx + r).ceil() as i32 + 1).min(self.size);
        let y1 = ((cy + r).ceil() as i32 + 1).min(self.size);
        for py in y0..y1 {
            for px in x0..x1 {
                let d = ((px as f32 + 0.5 - cx).hypot(py as f32 + 0.5 - cy)) - r;
                let cov = (0.5 - d).clamp(0.0, 1.0);
                if cov > 0.0 {
                    let a = ((col.alpha() as f32) * cov) as u8;
                    self.blend(px, py, col.with_alpha(a));
                }
            }
        }
    }
}

fn paint_master_32(id: IconId) -> MasterCanvas {
    let mut m = MasterCanvas::new(32);
    match id {
        IconId::StartFlag => {
            // Classic 4-pane wavy XP flag with subtle drop shadow
            let panes = [
                (4.0, 5.0, 10.5, 10.5, Color::rgb(243, 83, 37), Color::rgb(212, 52, 18)),
                (16.5, 6.5, 10.5, 10.5, Color::rgb(129, 188, 6), Color::rgb(88, 152, 4)),
                (4.0, 17.0, 10.5, 10.5, Color::rgb(5, 166, 240), Color::rgb(4, 122, 204)),
                (16.5, 18.5, 10.5, 10.5, Color::rgb(255, 186, 8), Color::rgb(228, 142, 4)),
            ];
            for (x, y, w, h, t, b) in panes {
                m.fill_rounded_grad(x + 1.0, y + 1.0, w, h, 2.0, Color::argb(90, 0, 0, 0), Color::argb(90, 0, 0, 0));
                m.fill_rounded_grad(x, y, w, h, 2.0, t, b);
                m.fill_rounded_grad(x + 1.0, y + 1.0, w - 2.0, h * 0.38, 1.5, Color::argb(70, 255, 255, 255), Color::argb(0, 255, 255, 255));
            }
        }
        IconId::Files => {
            // XP Luna golden folder with shadow, back tab, white paper sheet, and glossy front flap
            m.fill_rounded_grad(3.0, 24.0, 26.0, 4.0, 2.0, Color::argb(75, 0, 0, 0), Color::argb(0, 0, 0, 0));
            m.fill_rounded_grad(3.0, 5.0, 12.0, 6.0, 2.5, Color::rgb(228, 168, 42), Color::rgb(198, 138, 22));
            m.fill_rounded_grad(3.0, 8.0, 26.0, 18.0, 2.5, Color::rgb(245, 188, 56), Color::rgb(206, 144, 24));
            m.fill_rounded_grad(6.0, 9.5, 20.0, 6.0, 1.0, Color::rgb(252, 252, 255), Color::rgb(224, 232, 244));
            m.fill_rounded_grad(2.5, 12.0, 27.0, 15.0, 2.5, Color::rgb(255, 220, 98), Color::rgb(232, 166, 36));
            m.fill_rounded_grad(3.5, 12.5, 25.0, 4.0, 1.5, Color::argb(110, 255, 255, 255), Color::argb(0, 255, 255, 255));
        }
        IconId::Terminal => {
            // XP Command Prompt dark glass window
            m.fill_rounded_grad(3.0, 5.0, 26.0, 23.0, 3.0, Color::argb(80, 0, 0, 0), Color::argb(80, 0, 0, 0));
            m.fill_rounded_grad(2.0, 4.0, 26.0, 23.0, 3.0, Color::rgb(208, 214, 224), Color::rgb(118, 128, 144));
            m.fill_rounded_grad(3.0, 5.0, 24.0, 5.0, 2.0, Color::rgb(16, 102, 226), Color::rgb(8, 68, 180));
            m.fill_rounded_grad(3.5, 10.5, 23.0, 15.0, 1.5, Color::rgb(18, 24, 34), Color::rgb(10, 14, 20));
            // Prompt ">_"
            m.fill_rounded_grad(6.0, 14.0, 5.0, 2.0, 0.5, Color::rgb(96, 232, 130), Color::rgb(76, 210, 110));
            m.fill_rounded_grad(8.0, 16.0, 4.0, 2.0, 0.5, Color::rgb(96, 232, 130), Color::rgb(76, 210, 110));
            m.fill_rounded_grad(6.0, 18.0, 5.0, 2.0, 0.5, Color::rgb(96, 232, 130), Color::rgb(76, 210, 110));
            m.fill_rounded_grad(14.0, 19.0, 7.0, 2.0, 0.5, Color::rgb(228, 236, 246), Color::rgb(228, 236, 246));
        }
        IconId::Tasks => {
            // Task Manager green waveform monitor
            m.fill_rounded_grad(3.0, 5.0, 26.0, 22.0, 3.0, Color::rgb(198, 204, 216), Color::rgb(122, 132, 148));
            m.fill_rounded_grad(5.0, 7.0, 22.0, 18.0, 1.5, Color::rgb(10, 34, 22), Color::rgb(6, 22, 14));
            for i in 0..5 {
                let x = 7.0 + i as f32 * 4.0;
                let h = [6.0, 11.0, 8.0, 13.0, 7.0][i];
                m.fill_rounded_grad(x, 22.0 - h, 2.6, h, 1.0, Color::rgb(92, 242, 128), Color::rgb(34, 178, 72));
            }
        }
        IconId::Notepad => {
            // XP Notepad sheet + diagonal yellow pencil
            m.fill_rounded_grad(6.0, 4.0, 20.0, 25.0, 2.5, Color::argb(75, 0, 0, 0), Color::argb(75, 0, 0, 0));
            m.fill_rounded_grad(5.0, 3.0, 20.0, 25.0, 2.5, Color::rgb(156, 174, 198), Color::rgb(122, 140, 166));
            m.fill_rounded_grad(6.0, 4.0, 18.0, 23.0, 1.5, Color::rgb(255, 255, 252), Color::rgb(238, 242, 248));
            m.fill_rounded_grad(6.0, 4.0, 18.0, 4.0, 1.0, Color::rgb(46, 118, 214), Color::rgb(28, 88, 178));
            for row in 0..4 {
                m.fill_rounded_grad(8.5, 11.0 + row as f32 * 3.8, 12.0, 1.4, 0.5, Color::rgb(164, 180, 200), Color::rgb(164, 180, 200));
            }
            for step in 0..10 {
                let px = 17.0 + step as f32 * 0.9;
                let py = 19.0 - step as f32 * 0.9;
                m.fill_circle(px, py, 1.8, Color::rgb(246, 186, 36));
            }
        }
        IconId::Calc => {
            // XP Calculator
            m.fill_rounded_grad(6.0, 3.0, 20.0, 26.0, 3.0, Color::rgb(112, 142, 186), Color::rgb(64, 88, 126));
            m.fill_rounded_grad(8.0, 5.5, 16.0, 5.5, 1.0, Color::rgb(214, 242, 224), Color::rgb(184, 222, 198));
            for r in 0..3 {
                for c in 0..3 {
                    let col = if r == 2 && c == 2 {
                        Color::rgb(232, 112, 42)
                    } else {
                        Color::rgb(236, 240, 248)
                    };
                    m.fill_rounded_grad(8.5 + c as f32 * 5.2, 13.0 + r as f32 * 4.5, 4.0, 3.3, 0.8, col, col);
                }
            }
        }
        IconId::ControlPanel => {
            // XP Control Panel gear
            m.fill_circle(16.0, 16.0, 11.5, Color::rgb(108, 132, 166));
            for i in 0..8 {
                let ang = (i as f32) * core::f32::consts::TAU / 8.0;
                m.fill_circle(16.0 + ang.cos() * 11.0, 16.0 + ang.sin() * 11.0, 2.5, Color::rgb(138, 162, 196));
            }
            m.fill_circle(16.0, 16.0, 8.5, Color::rgb(198, 212, 232));
            m.fill_circle(16.0, 16.0, 4.2, Color::rgb(46, 122, 220));
        }
        IconId::MyComputer => {
            // XP My Computer LCD monitor + stand
            m.fill_rounded_grad(3.0, 4.0, 26.0, 19.0, 2.5, Color::rgb(168, 178, 192), Color::rgb(104, 114, 132));
            m.fill_rounded_grad(5.0, 6.0, 22.0, 14.0, 1.5, Color::rgb(42, 138, 242), Color::rgb(18, 78, 182));
            m.fill_rounded_grad(5.5, 6.5, 21.0, 5.0, 1.0, Color::argb(90, 255, 255, 255), Color::argb(0, 255, 255, 255));
            m.fill_rounded_grad(12.0, 23.0, 8.0, 3.0, 0.5, Color::rgb(138, 148, 162), Color::rgb(108, 118, 132));
            m.fill_rounded_grad(8.0, 26.0, 16.0, 2.8, 1.2, Color::rgb(164, 174, 188), Color::rgb(112, 122, 138));
        }
    }
    m
}

/// Draws an anti-aliased ARGB icon scaled cleanly to `size x size`, clipped to `canvas.bounds()`.
pub fn draw_icon(canvas: &mut dyn Canvas, x: i32, y: i32, size: i32, id: IconId) {
    let target = size.max(8);
    let master = paint_master_32(id);
    let clip = canvas.bounds();

    if target == 32 {
        canvas.blit_from(&master.px, 32, Rect::new(0, 0, 32, 32), x, y, clip, true);
        return;
    }

    let mut scaled = vec![0u32; (target * target) as usize];
    for dy in 0..target {
        let sy = (dy as f32 + 0.5) * 32.0 / (target as f32) - 0.5;
        let y0 = (sy.floor() as i32).clamp(0, 31);
        let y1 = (y0 + 1).min(31);
        let fy = (sy - y0 as f32).clamp(0.0, 1.0);

        for dx in 0..target {
            let sx = (dx as f32 + 0.5) * 32.0 / (target as f32) - 0.5;
            let x0 = (sx.floor() as i32).clamp(0, 31);
            let x1 = (x0 + 1).min(31);
            let fx = (sx - x0 as f32).clamp(0.0, 1.0);

            let c00 = Color(master.px[(y0 * 32 + x0) as usize]);
            let c10 = Color(master.px[(y0 * 32 + x1) as usize]);
            let c01 = Color(master.px[(y1 * 32 + x0) as usize]);
            let c11 = Color(master.px[(y1 * 32 + x1) as usize]);

            let w00 = (1.0 - fx) * (1.0 - fy);
            let w10 = fx * (1.0 - fy);
            let w01 = (1.0 - fx) * fy;
            let w11 = fx * fy;

            let a = (c00.alpha() as f32 * w00 + c10.alpha() as f32 * w10 + c01.alpha() as f32 * w01 + c11.alpha() as f32 * w11) as u8;
            let r = (c00.red() as f32 * w00 + c10.red() as f32 * w10 + c01.red() as f32 * w01 + c11.red() as f32 * w11) as u8;
            let g = (c00.green() as f32 * w00 + c10.green() as f32 * w10 + c01.green() as f32 * w01 + c11.green() as f32 * w11) as u8;
            let b = (c00.blue() as f32 * w00 + c10.blue() as f32 * w10 + c01.blue() as f32 * w01 + c11.blue() as f32 * w11) as u8;

            scaled[(dy * target + dx) as usize] = Color::argb(a, r, g, b).0;
        }
    }
    canvas.blit_from(&scaled, target as u32, Rect::new(0, 0, target, target), x, y, clip, true);
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
