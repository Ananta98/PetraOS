//! Centralized UI primitive painter (SerenityOS `LibGfx::StylePainter` architecture,
//! styled in PetraOS's crisp ReactOS-inspired Petra Slate aesthetic).

use crate::theme::Theme;
use crate::traits::WidgetState;
use space_core::font;
use space_core::{Canvas, Color, Rect};

/// Stateless theme-driven painter for standard UI chrome and widget primitives.
pub struct StylePainter<'a> {
    pub theme: &'a Theme,
}

impl<'a> StylePainter<'a> {
    #[inline]
    pub const fn new(theme: &'a Theme) -> Self {
        Self { theme }
    }

    /// Paints a subtle crisp frame (raised or inset) without heavy Win2000 double-bevels.
    pub fn paint_panel_3d(&self, canvas: &mut dyn Canvas, rect: Rect, sunken: bool) {
        if rect.w < 4 || rect.h < 4 {
            return;
        }
        let t = self.theme;
        let (tl, br) = if sunken {
            (t.bevel_shadow, t.bevel_light)
        } else {
            (t.bevel_light, t.bevel_shadow)
        };

        canvas.draw_rect_outline(rect, 1, t.border);
        canvas.fill_rect(Rect::new(rect.x + 1, rect.y + 1, rect.w - 2, 1), tl);
        canvas.fill_rect(Rect::new(rect.x + 1, rect.y + 1, 1, rect.h - 2), tl);
        canvas.fill_rect(Rect::new(rect.x + 1, rect.y + rect.h - 2, rect.w - 2, 1), br);
        canvas.fill_rect(Rect::new(rect.x + rect.w - 2, rect.y + 1, 1, rect.h - 2), br);
    }

    /// Paints a crisp modern-classic button background and 1px border according to `state`.
    pub fn paint_button_frame(
        &self,
        canvas: &mut dyn Canvas,
        rect: Rect,
        state: WidgetState,
        accent: bool,
    ) {
        let t = self.theme;
        let r = t.radius.min(rect.w / 2).min(rect.h / 2);
        let (top_c, bot_c, border_c) = if state == WidgetState::Disabled {
            (t.bg_window, t.bg_surface_alt, t.border)
        } else if accent {
            match state {
                WidgetState::Pressed => (
                    Color::lerp(t.accent, Color::BLACK, 1, 5),
                    t.accent,
                    t.bevel_dark,
                ),
                WidgetState::Hovered => (
                    Color::lerp(t.accent, Color::WHITE, 1, 3),
                    t.accent,
                    t.accent,
                ),
                _ => (
                    Color::lerp(t.accent, Color::WHITE, 1, 4),
                    Color::lerp(t.accent, Color::BLACK, 1, 6),
                    t.accent,
                ),
            }
        } else {
            match state {
                WidgetState::Pressed => (t.accent_soft, Color::lerp(t.accent_soft, t.accent, 1, 4), t.accent),
                WidgetState::Hovered => (t.bevel_light, t.bg_hover, t.accent),
                WidgetState::Focused => (t.bg_surface, t.bg_surface_alt, t.accent),
                _ => (t.bg_surface, Color::lerp(t.bg_window, t.bevel_shadow, 1, 4), t.border),
            }
        };

        canvas.fill_rounded_rect(rect, r, bot_c);
        if t.subtle_gradients && rect.w > 2 && rect.h > 2 {
            let inner = Rect::new(rect.x + 1, rect.y + 1, rect.w - 2, rect.h - 2);
            canvas.fill_gradient_v(inner, top_c, bot_c);
            canvas.draw_rect_outline(rect, 1, border_c);
            if state == WidgetState::Normal && !accent {
                canvas.fill_rect(Rect::new(inner.x, inner.y, inner.w, 1), t.bevel_light);
            } else if state == WidgetState::Hovered || state == WidgetState::Focused {
                canvas.draw_rect_outline(inner, 1, t.accent_soft);
            }
        } else {
            canvas.draw_rect_outline(rect, 1, border_c);
        }
    }

    /// Paints a sunken input/list container background with crisp 1px border and focus ring.
    pub fn paint_sunken_field(&self, canvas: &mut dyn Canvas, rect: Rect, focused: bool) {
        let t = self.theme;
        canvas.fill_rect(rect, t.bg_input);
        if rect.w > 2 && rect.h > 2 {
            canvas.fill_rect(Rect::new(rect.x + 1, rect.y + 1, rect.w - 2, 1), t.bevel_shadow);
            canvas.fill_rect(Rect::new(rect.x + 1, rect.y + 1, 1, rect.h - 2), t.bevel_shadow);
        }
        let border = if focused { t.accent } else { t.border };
        canvas.draw_rect_outline(rect, 1, border);
    }

    /// Paints a clean toolbar/menubar/statusbar strip with subtle top highlight and bottom rule.
    pub fn paint_raised_bar(&self, canvas: &mut dyn Canvas, rect: Rect) {
        let t = self.theme;
        canvas.fill_gradient_v(rect, t.bg_surface, t.bg_window);
        canvas.fill_rect(Rect::new(rect.x, rect.y, rect.w, 1), t.bevel_light);
        canvas.fill_rect(Rect::new(rect.x, rect.y + rect.h - 1, rect.w, 1), t.bevel_shadow);
    }

    /// Paints a vertical or horizontal subtle 1px-etched divider line.
    pub fn paint_divider(&self, canvas: &mut dyn Canvas, rect: Rect, vertical: bool) {
        let t = self.theme;
        if vertical {
            canvas.fill_rect(Rect::new(rect.x, rect.y, 1, rect.h), t.bevel_shadow);
            canvas.fill_rect(Rect::new(rect.x + 1, rect.y, 1, rect.h), t.bevel_light);
        } else {
            canvas.fill_rect(Rect::new(rect.x, rect.y, rect.w, 1), t.bevel_shadow);
            canvas.fill_rect(Rect::new(rect.x, rect.y + 1, rect.w, 1), t.bevel_light);
        }
    }

    /// Paints a list/table row background (alternating stripe or active selection).
    pub fn paint_row_bg(&self, canvas: &mut dyn Canvas, rect: Rect, row_idx: usize, selected: bool) {
        let t = self.theme;
        let bg = if selected {
            t.bg_active
        } else if row_idx % 2 == 1 {
            t.bg_surface_alt
        } else {
            t.bg_input
        };
        canvas.fill_rect(rect, bg);
    }

    /// Paints a progress bar inside `rect`.
    pub fn paint_progress_bar(&self, canvas: &mut dyn Canvas, rect: Rect, ratio: f32, label: &str, fill: Color) {
        let t = self.theme;
        self.paint_sunken_field(canvas, rect, false);
        let fill_w = ((rect.w - 4).max(0) as f32 * ratio.clamp(0.0, 1.0)) as i32;
        if fill_w > 0 {
            let fill_r = Rect::new(rect.x + 2, rect.y + 2, fill_w, (rect.h - 4).max(0));
            canvas.fill_gradient_v(fill_r, Color::lerp(fill, Color::WHITE, 1, 3), fill);
        }
        if !label.is_empty() {
            let ty = rect.y + (rect.h - font::GLYPH_H * 2) / 2;
            font::draw_text(canvas, rect.x + 8, ty, label, t.text_primary, 2);
        }
    }
}
