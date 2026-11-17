//! Alt+Tab visual WindowSwitcher overlay (SerenityOS `WindowServer/WindowSwitcher` pattern).

use crate::scene::SceneGraph;
use space_core::font;
use space_core::icons::{self, IconId};
use space_core::{Canvas, Color, Rect};
use space_ui::{StylePainter, Theme};

pub struct WindowSwitcher {
    pub active: bool,
    pub selected: usize,
    pub entries: Vec<(u32, String)>,
}

impl WindowSwitcher {
    pub const fn new() -> Self {
        Self {
            active: false,
            selected: 0,
            entries: Vec::new(),
        }
    }

    /// Opens or advances the switcher through windows on the current workspace.
    pub fn step(&mut self, scene: &SceneGraph, reverse: bool) {
        if !self.active {
            let ws = scene.active_workspace;
            self.entries = scene
                .z_order
                .iter()
                .rev()
                .filter_map(|&id| {
                    let n = scene.find(id)?;
                    if n.workspace == ws {
                        Some((n.id, n.title.clone()))
                    } else {
                        None
                    }
                })
                .collect();
            if self.entries.is_empty() {
                return;
            }
            self.active = true;
            self.selected = if self.entries.len() > 1 { 1 } else { 0 };
        } else if !self.entries.is_empty() {
            let len = self.entries.len();
            self.selected = if reverse {
                (self.selected + len - 1) % len
            } else {
                (self.selected + 1) % len
            };
        }
    }

    /// Commits the selected window ID and closes the overlay.
    pub fn commit(&mut self) -> Option<u32> {
        if !self.active {
            return None;
        }
        self.active = false;
        let id = self.entries.get(self.selected).map(|(id, _)| *id);
        self.entries.clear();
        id
    }

    pub fn overlay_rect(&self, sw: i32, sh: i32) -> Rect {
        let count = self.entries.len().max(1) as i32;
        let w = (count * 124 + 28).min(sw - 40).max(220);
        let h = 112;
        Rect::new((sw - w) / 2, (sh - h) / 2, w + 4, h + 4)
    }

    pub fn paint(&self, canvas: &mut dyn Canvas, sw: i32, sh: i32) {
        if !self.active || self.entries.is_empty() {
            return;
        }
        let t = Theme::PETRA_SLATE;
        let full = self.overlay_rect(sw, sh);
        let r = Rect::new(full.x, full.y, full.w - 4, full.h - 4);
        canvas.fill_rect(Rect::new(r.x + 4, r.y + 4, r.w, r.h), Color::argb(80, 0, 0, 0));
        canvas.fill_rounded_rect(r, 4, t.bg_window);
        StylePainter::new(&t).paint_panel_3d(canvas, r, false);
        canvas.draw_rect_outline(r, 1, t.accent);

        let mut x = r.x + 14;
        for (i, (_, title)) in self.entries.iter().enumerate() {
            let card = Rect::new(x, r.y + 14, 112, 84);
            if i == self.selected {
                canvas.fill_rounded_rect(card, 3, t.accent);
                canvas.draw_rect_outline(card, 1, Theme::TITLE_ACTIVE_HI);
            } else {
                canvas.fill_rounded_rect(card, 3, t.bg_surface);
                canvas.draw_rect_outline(card, 1, t.border);
            }
            let icon = IconId::from_title(title);
            icons::draw_icon(canvas, card.x + (card.w - 32) / 2, card.y + 12, 32, &icon);
            let short: String = title.chars().take(12).collect();
            let lw = font::measure_text_width(&short, 1);
            let fg = if i == self.selected { t.text_on_accent } else { t.text_primary };
            font::draw_text(canvas, card.x + (card.w - lw) / 2, card.y + 54, &short, fg, 1);
            x += 120;
            if x + 112 > r.x + r.w {
                break;
            }
        }
    }
}
