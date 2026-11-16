//! Window decoration abstraction (OCP/LSP) with Windows XP (Luna Blue) & Breeze styling.

use space_core::font;
use space_core::icons::{self, IconId, WindowGlyph};
use space_core::{Canvas, Color, Point, Rect};
use space_ui::Theme;

pub const TITLEBAR_H: i32 = 30;
pub const BORDER_W: i32 = 4;
pub const SHADOW_SIZE: i32 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitPart {
    None,
    ClientArea,
    TitleBar,
    CloseButton,
    MaximizeButton,
    MinimizeButton,
    ResizeCorner,
}

/// Polymorphic window decorator contract (LSP/OCP).
pub trait WindowDecorator {
    fn outer_rect(&self, client_rect: Rect) -> Rect;
    fn shadow_rect(&self, client_rect: Rect) -> Rect;
    fn hit_test(&self, client_rect: Rect, pos: Point) -> HitPart;
    fn paint_frame(&self, canvas: &mut dyn Canvas, client_rect: Rect, title: &str, focused: bool, clip: Rect);
}

pub struct XpLunaDecorator;

impl XpLunaDecorator {
    fn button_rects(outer: Rect) -> (Rect, Rect, Rect) {
        let by = outer.y + 5;
        let bs = 21;
        let close = Rect::new(outer.right() - 26, by, bs, bs);
        let max = Rect::new(outer.right() - 49, by, bs, bs);
        let min = Rect::new(outer.right() - 72, by, bs, bs);
        (min, max, close)
    }

    fn draw_xp_button(canvas: &mut dyn Canvas, r: Rect, bg: Color, dark_sh: Color, glyph: WindowGlyph) {
        let top_hi = Color::lerp(bg, Color::WHITE, 2, 5);
        let bot_lo = Color::lerp(bg, dark_sh, 2, 5);
        let inner = Rect::new(r.x + 1, r.y + 1, r.w - 2, r.h - 2);
        canvas.fill_gradient_v(inner, top_hi, bot_lo);
        // Classic XP white rounded-square button border
        canvas.fill_rect(Rect::new(r.x + 1, r.y, r.w - 2, 1), Color::WHITE);
        canvas.fill_rect(Rect::new(r.x + 1, r.bottom() - 1, r.w - 2, 1), Color::WHITE);
        canvas.fill_rect(Rect::new(r.x, r.y + 1, 1, r.h - 2), Color::WHITE);
        canvas.fill_rect(Rect::new(r.right() - 1, r.y + 1, 1, r.h - 2), Color::WHITE);
        // Subtle inner bottom-right shadow
        canvas.fill_rect(Rect::new(r.x + 1, r.bottom() - 2, r.w - 2, 1), dark_sh);
        canvas.fill_rect(Rect::new(r.right() - 2, r.y + 1, 1, r.h - 2), dark_sh);
        icons::draw_window_glyph(canvas, r, glyph, Color::WHITE);
    }
}

impl WindowDecorator for XpLunaDecorator {
    #[inline]
    fn outer_rect(&self, client_rect: Rect) -> Rect {
        Rect::new(
            client_rect.x - BORDER_W,
            client_rect.y - TITLEBAR_H,
            client_rect.w + BORDER_W * 2,
            client_rect.h + TITLEBAR_H + BORDER_W,
        )
    }

    #[inline]
    fn shadow_rect(&self, client_rect: Rect) -> Rect {
        let o = self.outer_rect(client_rect);
        Rect::new(o.x - 1, o.y - 1, o.w + SHADOW_SIZE + 2, o.h + SHADOW_SIZE + 2)
    }

    fn hit_test(&self, client_rect: Rect, pos: Point) -> HitPart {
        let outer = self.outer_rect(client_rect);
        if !outer.contains(pos) {
            return HitPart::None;
        }
        let resize_corner = Rect::new(outer.right() - 14, outer.bottom() - 14, 14, 14);
        if resize_corner.contains(pos) {
            return HitPart::ResizeCorner;
        }
        let (min_r, max_r, close_r) = Self::button_rects(outer);
        if close_r.contains(pos) {
            return HitPart::CloseButton;
        }
        if max_r.contains(pos) {
            return HitPart::MaximizeButton;
        }
        if min_r.contains(pos) {
            return HitPart::MinimizeButton;
        }
        let title_r = Rect::new(outer.x, outer.y, outer.w, TITLEBAR_H);
        if title_r.contains(pos) {
            return HitPart::TitleBar;
        }
        if client_rect.contains(pos) {
            return HitPart::ClientArea;
        }
        HitPart::None
    }

    fn paint_frame(&self, canvas: &mut dyn Canvas, client_rect: Rect, title: &str, focused: bool, clip: Rect) {
        let outer = self.outer_rect(client_rect);

        // Soft drop shadow
        let sh1 = Rect::new(outer.x + 4, outer.y + 4, outer.w, outer.h).intersect(clip);
        canvas.fill_rect(sh1, Color::argb(65, 0, 0, 0));
        let sh2 = Rect::new(outer.x + 2, outer.y + 2, outer.w, outer.h).intersect(clip);
        canvas.fill_rect(sh2, Color::argb(85, 0, 0, 0));

        let border_outer = if focused {
            Theme::XP_TITLE_ACTIVE_BORDER
        } else {
            Color::rgb(0x6C, 0x87, 0xC9)
        };
        let border_inner = if focused {
            Theme::XP_TITLE_ACTIVE_TOP
        } else {
            Theme::XP_TITLE_INACTIVE_TOP
        };

        // Rounded top-corner glossy Luna titlebar
        let r = 6i32;
        for row in 0..TITLEBAR_H {
            let py = outer.y + row;
            if py < clip.y || py >= clip.bottom() {
                continue;
            }
            let inset = if row < r {
                let dy = r - row;
                let dx = ((r * r - dy * dy) as f32).sqrt() as i32;
                (r - dx).max(0)
            } else {
                0
            };
            let rx = outer.x + inset;
            let rw = (outer.w - inset * 2).max(0);
            if rw <= 0 {
                continue;
            }
            let row_col = if row == 0 {
                border_outer
            } else if focused {
                if row < 7 {
                    Color::lerp(Theme::XP_TITLE_ACTIVE_TOP, Theme::XP_TITLE_ACTIVE_MID, row - 1, 6)
                } else {
                    Color::lerp(Theme::XP_TITLE_ACTIVE_MID, Theme::XP_TITLE_ACTIVE_BOT, row - 7, TITLEBAR_H - 8)
                }
            } else {
                Color::lerp(Theme::XP_TITLE_INACTIVE_TOP, Theme::XP_TITLE_INACTIVE_BOT, row - 1, TITLEBAR_H - 2)
            };
            canvas.fill_rect(Rect::new(rx, py, rw, 1), row_col);
            if row > 0 {
                canvas.fill_rect(Rect::new(rx, py, 1, 1), border_outer);
                canvas.fill_rect(Rect::new(rx + rw - 1, py, 1, 1), border_outer);
            }
        }

        // Left, right, and bottom window borders
        let body_y = outer.y + TITLEBAR_H;
        let body_h = outer.h - TITLEBAR_H;
        if body_h > 0 {
            canvas.fill_rect(Rect::new(outer.x, body_y, BORDER_W, body_h), border_inner);
            canvas.fill_rect(Rect::new(outer.right() - BORDER_W, body_y, BORDER_W, body_h), border_inner);
            canvas.fill_rect(Rect::new(outer.x, outer.bottom() - BORDER_W, outer.w, BORDER_W), border_inner);
            // 1px outer frame line
            canvas.fill_rect(Rect::new(outer.x, body_y, 1, body_h), border_outer);
            canvas.fill_rect(Rect::new(outer.right() - 1, body_y, 1, body_h), border_outer);
            canvas.fill_rect(Rect::new(outer.x, outer.bottom() - 1, outer.w, 1), border_outer);
        }

        // Client area fallback fill (prevents stale pixels if window is larger than surface)
        let client_vis = client_rect.intersect(clip);
        if !client_vis.is_empty() {
            canvas.fill_rect(client_vis, Theme::XP_LUNA.bg_window);
        }

        // 18x18 app icon on the left
        icons::draw_icon(canvas, outer.x + 7, outer.y + 6, 18, IconId::from_title(title));

        // Bold white title text with subtle drop shadow
        let max_title_chars = ((outer.w - 110).max(12) / 8) as usize;
        let clipped_title: String = title.chars().take(max_title_chars).collect();
        let text_col = if focused {
            Color::WHITE
        } else {
            Color::rgb(225, 234, 248)
        };
        let sh_col = if focused {
            Color::rgb(0, 28, 108)
        } else {
            Color::rgb(72, 92, 138)
        };
        font::draw_text_shadow(canvas, outer.x + 30, outer.y + 7, &clipped_title, text_col, sh_col, 2, true);

        // Raised Min / Max / Close buttons
        let (min_r, max_r, close_r) = Self::button_rects(outer);
        let btn_blue = if focused { Theme::XP_BTN_MINMAX } else { Theme::XP_TITLE_INACTIVE_TOP };
        let btn_blue_sh = if focused { Theme::XP_TITLE_ACTIVE_BORDER } else { Color::rgb(0x5A, 0x76, 0xB8) };
        if !min_r.intersect(clip).is_empty() {
            Self::draw_xp_button(canvas, min_r, btn_blue, btn_blue_sh, WindowGlyph::Minimize);
        }
        if !max_r.intersect(clip).is_empty() {
            Self::draw_xp_button(canvas, max_r, btn_blue, btn_blue_sh, WindowGlyph::Maximize);
        }
        if !close_r.intersect(clip).is_empty() {
            let close_col = if focused { Theme::XP_LUNA.danger } else { Color::rgb(0xC8, 0x70, 0x60) };
            Self::draw_xp_button(canvas, close_r, close_col, Color::rgb(0x9E, 0x28, 0x10), WindowGlyph::Close);
        }
    }
}

#[allow(dead_code)]
pub struct BreezeXfceDecorator;

#[allow(dead_code)]
impl BreezeXfceDecorator {
    fn button_rects(outer: Rect) -> (Rect, Rect, Rect) {
        let by = outer.y + 5;
        let bs = 18;
        let close = Rect::new(outer.right() - 24, by, bs, bs);
        let max = Rect::new(outer.right() - 46, by, bs, bs);
        let min = Rect::new(outer.right() - 68, by, bs, bs);
        (min, max, close)
    }
}

impl WindowDecorator for BreezeXfceDecorator {
    #[inline]
    fn outer_rect(&self, client_rect: Rect) -> Rect {
        Rect::new(
            client_rect.x - 2,
            client_rect.y - TITLEBAR_H,
            client_rect.w + 4,
            client_rect.h + TITLEBAR_H + 2,
        )
    }

    #[inline]
    fn shadow_rect(&self, client_rect: Rect) -> Rect {
        let o = self.outer_rect(client_rect);
        Rect::new(o.x - 2, o.y - 1, o.w + 8, o.h + 8)
    }

    fn hit_test(&self, client_rect: Rect, pos: Point) -> HitPart {
        let outer = self.outer_rect(client_rect);
        if !outer.contains(pos) {
            return HitPart::None;
        }
        let resize_corner = Rect::new(outer.right() - 14, outer.bottom() - 14, 14, 14);
        if resize_corner.contains(pos) {
            return HitPart::ResizeCorner;
        }
        let (min_r, max_r, close_r) = Self::button_rects(outer);
        if close_r.contains(pos) {
            return HitPart::CloseButton;
        }
        if max_r.contains(pos) {
            return HitPart::MaximizeButton;
        }
        if min_r.contains(pos) {
            return HitPart::MinimizeButton;
        }
        let title_r = Rect::new(outer.x, outer.y, outer.w, TITLEBAR_H);
        if title_r.contains(pos) {
            return HitPart::TitleBar;
        }
        if client_rect.contains(pos) {
            return HitPart::ClientArea;
        }
        HitPart::None
    }

    fn paint_frame(&self, canvas: &mut dyn Canvas, client_rect: Rect, title: &str, focused: bool, clip: Rect) {
        let t = Theme::BREEZE_DARK;
        let outer = self.outer_rect(client_rect);

        let sh1 = Rect::new(outer.x + 4, outer.y + 4, outer.w, outer.h).intersect(clip);
        canvas.fill_rect(sh1, Color::argb(70, 0, 0, 0));
        let sh2 = Rect::new(outer.x + 2, outer.y + 2, outer.w, outer.h).intersect(clip);
        canvas.fill_rect(sh2, Color::argb(90, 0, 0, 0));

        let title_bg = if focused { Color::rgb(38, 44, 56) } else { Color::rgb(28, 32, 40) };
        let border_col = if focused { t.accent } else { t.border };

        let frame_clip = outer.intersect(clip);
        if !frame_clip.is_empty() {
            canvas.fill_rect(Rect::new(outer.x, outer.y, outer.w, TITLEBAR_H).intersect(clip), title_bg);
            canvas.draw_rect_outline(outer, 2, border_col);
            if focused {
                canvas.fill_rect(
                    Rect::new(outer.x + 2, outer.y + TITLEBAR_H - 2, outer.w - 4, 2).intersect(clip),
                    t.accent_soft,
                );
            }
        }

        let text_col = if focused { t.text_primary } else { t.text_secondary };
        font::draw_text(canvas, outer.x + 10, outer.y + 7, title, text_col, 2);

        let (min_r, max_r, close_r) = Self::button_rects(outer);
        if !min_r.intersect(clip).is_empty() {
            canvas.fill_rounded_rect(min_r, 4, t.bg_surface_alt);
            canvas.fill_rect(Rect::new(min_r.x + 4, min_r.y + 12, 10, 2), t.text_primary);
        }
        if !max_r.intersect(clip).is_empty() {
            canvas.fill_rounded_rect(max_r, 4, t.bg_surface_alt);
            canvas.draw_rect_outline(Rect::new(max_r.x + 4, max_r.y + 4, 10, 10), 1, t.text_primary);
        }
        if !close_r.intersect(clip).is_empty() {
            canvas.fill_rounded_rect(close_r, 4, t.danger);
            font::draw_text(canvas, close_r.x + 4, close_r.y + 3, "x", Color::WHITE, 2);
        }
    }
}
