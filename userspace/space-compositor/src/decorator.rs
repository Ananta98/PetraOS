//! Window decoration abstraction (OCP/LSP) with Breeze/XFCE-inspired styling.

use space_core::font;
use space_core::{Canvas, Color, Point, Rect};
use space_ui::Theme;

pub const TITLEBAR_H: i32 = 28;
pub const BORDER_W: i32 = 2;
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

pub struct BreezeXfceDecorator;

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
            client_rect.x - BORDER_W,
            client_rect.y - TITLEBAR_H,
            client_rect.w + BORDER_W * 2,
            client_rect.h + TITLEBAR_H + BORDER_W,
        )
    }

    #[inline]
    fn shadow_rect(&self, client_rect: Rect) -> Rect {
        let o = self.outer_rect(client_rect);
        Rect::new(o.x - 2, o.y - 1, o.w + SHADOW_SIZE + 2, o.h + SHADOW_SIZE + 2)
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

        // Soft drop shadow (2-layer alpha)
        let sh1 = Rect::new(outer.x + 4, outer.y + 4, outer.w, outer.h).intersect(clip);
        canvas.fill_rect(sh1, Color::argb(70, 0, 0, 0));
        let sh2 = Rect::new(outer.x + 2, outer.y + 2, outer.w, outer.h).intersect(clip);
        canvas.fill_rect(sh2, Color::argb(90, 0, 0, 0));

        // Window border + titlebar background
        let title_bg = if focused {
            Color::rgb(38, 44, 56)
        } else {
            Color::rgb(28, 32, 40)
        };
        let border_col = if focused { t.accent } else { t.border };

        let frame_clip = outer.intersect(clip);
        if !frame_clip.is_empty() {
            canvas.fill_rect(Rect::new(outer.x, outer.y, outer.w, TITLEBAR_H).intersect(clip), title_bg);
            canvas.draw_rect_outline(outer, BORDER_W, border_col);
            if focused {
                canvas.fill_rect(
                    Rect::new(outer.x + 2, outer.y + TITLEBAR_H - 2, outer.w - 4, 2).intersect(clip),
                    t.accent_soft,
                );
            }
        }

        // Title text
        let text_col = if focused { t.text_primary } else { t.text_secondary };
        font::draw_text(canvas, outer.x + 10, outer.y + 7, title, text_col, 2);

        // Window control buttons
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
