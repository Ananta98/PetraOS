//! Window decoration abstraction with Petra Slate (ReactOS-inspired modern-classic) styling.

use space_core::font;
use space_core::icons::{self, IconId, WindowGlyph};
use space_core::{Canvas, Color, Point, Rect};
use space_ui::Theme;

pub const TITLEBAR_H: i32 = 28;
pub const BORDER_W: i32 = 3;
pub const SHADOW_SIZE: i32 = 5;

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

/// Polymorphic window decorator contract.
pub trait WindowDecorator {
    fn outer_rect(&self, client_rect: Rect) -> Rect;
    fn shadow_rect(&self, client_rect: Rect) -> Rect;
    fn hit_test(&self, client_rect: Rect, pos: Point) -> HitPart;
    fn paint_frame(
        &self,
        canvas: &mut dyn Canvas,
        client_rect: Rect,
        title: &str,
        focused: bool,
        maximized: bool,
        clip: Rect,
    );
}

/// Crisp ReactOS-inspired modern-classic window frame for Petra Slate.
pub struct PetraDecorator;

impl PetraDecorator {
    fn button_rects(outer: Rect) -> (Rect, Rect, Rect) {
        let by = outer.y + 4;
        let bw = 20;
        let bh = 19;
        let close = Rect::new(outer.right() - 24, by, bw, bh);
        let max = Rect::new(outer.right() - 46, by, bw, bh);
        let min = Rect::new(outer.right() - 68, by, bw, bh);
        (min, max, close)
    }

    fn draw_title_button(
        canvas: &mut dyn Canvas,
        r: Rect,
        top: Color,
        bot: Color,
        border: Color,
        glyph_col: Color,
        glyph: WindowGlyph,
    ) {
        let inner = Rect::new(r.x + 1, r.y + 1, (r.w - 2).max(0), (r.h - 2).max(0));
        canvas.fill_gradient_v(inner, top, bot);
        canvas.draw_rect_outline(r, 1, border);
        icons::draw_window_glyph(canvas, r, glyph, glyph_col);
    }
}

impl WindowDecorator for PetraDecorator {
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

    fn paint_frame(
        &self,
        canvas: &mut dyn Canvas,
        client_rect: Rect,
        title: &str,
        focused: bool,
        maximized: bool,
        clip: Rect,
    ) {
        let outer = self.outer_rect(client_rect);
        let t = Theme::PETRA_SLATE;

        // Crisp subtle drop shadow
        let sh1 = Rect::new(outer.x + 3, outer.y + 3, outer.w, outer.h).intersect(clip);
        canvas.fill_rect(sh1, Color::argb(55, 0, 0, 0));
        let sh2 = Rect::new(outer.x + 1, outer.y + 1, outer.w, outer.h).intersect(clip);
        canvas.fill_rect(sh2, Color::argb(70, 0, 0, 0));

        let border_outer = if focused {
            Theme::TITLE_ACTIVE_BORDER
        } else {
            Theme::TITLE_INACTIVE_BORDER
        };
        let border_fill = if focused {
            Theme::TITLE_ACTIVE_BOT
        } else {
            Theme::TITLE_INACTIVE_BOT
        };
        let (title_top, title_bot, title_hi) = if focused {
            (Theme::TITLE_ACTIVE_TOP, Theme::TITLE_ACTIVE_BOT, Theme::TITLE_ACTIVE_HI)
        } else {
            (Theme::TITLE_INACTIVE_TOP, Theme::TITLE_INACTIVE_BOT, Color::rgb(0xEC, 0xF0, 0xF5))
        };

        // Crisp titlebar with 1px outer frame & subtle top inner highlight
        let title_r = Rect::new(outer.x + 1, outer.y + 1, (outer.w - 2).max(0), TITLEBAR_H - 1);
        let title_vis = title_r.intersect(clip);
        if !title_vis.is_empty() {
            canvas.fill_gradient_v(title_r, title_top, title_bot);
            canvas.fill_rect(Rect::new(outer.x + 1, outer.y + 1, (outer.w - 2).max(0), 1), title_hi);
        }

        // Left, right, and bottom window borders
        let body_y = outer.y + TITLEBAR_H;
        let body_h = outer.h - TITLEBAR_H;
        if body_h > 0 {
            canvas.fill_rect(Rect::new(outer.x, body_y, BORDER_W, body_h), border_fill);
            canvas.fill_rect(Rect::new(outer.right() - BORDER_W, body_y, BORDER_W, body_h), border_fill);
            canvas.fill_rect(Rect::new(outer.x, outer.bottom() - BORDER_W, outer.w, BORDER_W), border_fill);
        }

        // Crisp 1px outer frame outline
        canvas.draw_rect_outline(outer, 1, border_outer);

        // Client area fallback fill
        let client_vis = client_rect.intersect(clip);
        if !client_vis.is_empty() {
            canvas.fill_rect(client_vis, t.bg_window);
        }

        // 16x16 app icon on the left
        icons::draw_icon(canvas, outer.x + 7, outer.y + 6, 16, &IconId::from_title(title));

        // Clean title text
        let max_title_chars = ((outer.w - 104).max(12) / 8) as usize;
        let clipped_title: String = title.chars().take(max_title_chars).collect();
        let text_col = if focused {
            Color::WHITE
        } else {
            Color::rgb(0x3D, 0x47, 0x56)
        };
        if focused {
            font::draw_text_shadow(
                canvas,
                outer.x + 28,
                outer.y + 6,
                &clipped_title,
                text_col,
                Color::rgb(0x10, 0x26, 0x4C),
                2,
                true,
            );
        } else {
            font::draw_text(canvas, outer.x + 28, outer.y + 6, &clipped_title, text_col, 2);
        }

        // Clean ReactOS-inspired flat/subtle-gradient Min / Max / Close buttons
        let (min_r, max_r, close_r) = Self::button_rects(outer);
        let (btn_top, btn_bot, btn_border, glyph_col) = if focused {
            (
                Color::rgb(0x52, 0x89, 0xCF),
                Theme::TITLE_BTN_BG,
                Theme::TITLE_ACTIVE_BORDER,
                Color::WHITE,
            )
        } else {
            (
                Color::rgb(0xE8, 0xEC, 0xF2),
                Color::rgb(0xCD, 0xD5, 0xE0),
                Theme::TITLE_INACTIVE_BORDER,
                Color::rgb(0x46, 0x52, 0x62),
            )
        };

        if !min_r.intersect(clip).is_empty() {
            Self::draw_title_button(canvas, min_r, btn_top, btn_bot, btn_border, glyph_col, WindowGlyph::Minimize);
        }
        if !max_r.intersect(clip).is_empty() {
            let max_glyph = if maximized { WindowGlyph::Restore } else { WindowGlyph::Maximize };
            Self::draw_title_button(canvas, max_r, btn_top, btn_bot, btn_border, glyph_col, max_glyph);
        }
        if !close_r.intersect(clip).is_empty() {
            let (cl_top, cl_bot, cl_border, cl_glyph) = if focused {
                (
                    Color::rgb(0xDF, 0x5A, 0x5A),
                    t.danger,
                    Color::rgb(0x8E, 0x22, 0x22),
                    Color::WHITE,
                )
            } else {
                (btn_top, btn_bot, btn_border, glyph_col)
            };
            Self::draw_title_button(canvas, close_r, cl_top, cl_bot, cl_border, cl_glyph, WindowGlyph::Close);
        }
    }
}
