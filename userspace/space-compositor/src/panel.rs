//! Classic Windows XP (Luna Blue) bottom taskbar, system tray, and two-column Start Menu.

use crate::scene::{LayoutMode, SceneGraph};
use space_core::font;
use space_core::icons::{self, IconId};
use space_core::{Canvas, Color, Point, Rect};
use space_ui::Theme;

pub const TASKBAR_H: i32 = 34;

#[derive(Debug, Clone, Copy)]
pub struct AppEntry {
    pub name: &'static str,
    pub binary: &'static str,
    pub icon: IconId,
}

pub const BUILTIN_APPS: [AppEntry; 6] = [
    AppEntry {
        name: "Files",
        binary: "space-files",
        icon: IconId::Files,
    },
    AppEntry {
        name: "Terminal",
        binary: "space-term",
        icon: IconId::Terminal,
    },
    AppEntry {
        name: "Tasks",
        binary: "space-tasks",
        icon: IconId::Tasks,
    },
    AppEntry {
        name: "Notepad",
        binary: "space-pad",
        icon: IconId::Notepad,
    },
    AppEntry {
        name: "Calc",
        binary: "space-calc",
        icon: IconId::Calc,
    },
    AppEntry {
        name: "Control Panel",
        binary: "space-control",
        icon: IconId::ControlPanel,
    },
];

const QUICK_LAUNCH: [(&str, IconId); 3] = [
    ("space-files", IconId::Files),
    ("space-term", IconId::Terminal),
    ("space-pad", IconId::Notepad),
];

const START_PLACES: [(&str, &str, IconId); 4] = [
    ("My Computer", "space-files", IconId::MyComputer),
    ("Files", "space-files", IconId::Files),
    ("Task Manager", "space-tasks", IconId::Tasks),
    ("Control Panel", "space-control", IconId::ControlPanel),
];

pub enum PanelAction {
    None,
    ToggleLauncher,
    SwitchWorkspace(u8),
    ToggleLayout,
    FocusWindow(u32),
    LaunchApp(&'static str),
}

pub struct DesktopPanel {
    pub launcher_open: bool,
    pub mem_mb: u32,
    pub mem_total_mb: u32,
}

impl DesktopPanel {
    pub fn new() -> Self {
        Self {
            launcher_open: false,
            mem_mb: 0,
            mem_total_mb: 0,
        }
    }

    pub fn update_stats(&mut self) {
        if let Ok(meminfo) = std::fs::read_to_string("/proc/meminfo") {
            let mut total_kb = 0u64;
            let mut avail_kb = 0u64;
            for line in meminfo.lines() {
                if let Some(rest) = line.strip_prefix("MemTotal:") {
                    total_kb = rest.split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(0);
                } else if let Some(rest) = line.strip_prefix("MemAvailable:") {
                    avail_kb = rest.split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(0);
                }
            }
            if total_kb > 0 {
                self.mem_total_mb = (total_kb / 1024) as u32;
                self.mem_mb = ((total_kb.saturating_sub(avail_kb)) / 1024) as u32;
            }
        }
    }

    #[inline]
    pub fn work_area(&self, screen_w: i32, screen_h: i32) -> Rect {
        Rect::new(0, 0, screen_w, screen_h - TASKBAR_H)
    }

    #[inline]
    pub fn panel_rect(&self, screen_w: i32, screen_h: i32) -> Rect {
        Rect::new(0, screen_h - TASKBAR_H, screen_w, TASKBAR_H)
    }

    #[inline]
    pub fn launcher_rect(&self, screen_h: i32) -> Rect {
        Rect::new(0, screen_h - TASKBAR_H - 310, 412, 310)
    }

    fn tray_width(&self) -> i32 {
        let stats = format!("RAM {}M/{}M", self.mem_mb, self.mem_total_mb);
        let sw = font::measure_text_width(&stats, 2);
        // 4 workspaces (4*22=88) + layout (68) + stats + padding
        88 + 68 + sw + 30
    }

    fn start_button_rect(&self, screen_h: i32) -> Rect {
        Rect::new(0, screen_h - TASKBAR_H, 102, TASKBAR_H)
    }

    pub fn paint(&self, canvas: &mut dyn Canvas, scene: &SceneGraph, screen_w: i32, screen_h: i32) {
        let tb_y = screen_h - TASKBAR_H;
        let panel_r = self.panel_rect(screen_w, screen_h);

        // 1. Taskbar Luna Blue Gradient
        canvas.fill_rect(Rect::new(0, tb_y, screen_w, 2), Theme::XP_TASKBAR_TOP_HI);
        canvas.fill_gradient_v(
            Rect::new(0, tb_y + 2, screen_w, TASKBAR_H - 2),
            Theme::XP_TASKBAR_BLUE,
            Theme::XP_TASKBAR_BOT,
        );

        // 2. Green Rounded Start Button
        let start_r = self.start_button_rect(screen_h);
        let (st_top, st_bot) = if self.launcher_open {
            (Color::rgb(0x2B, 0x7A, 0x2B), Color::rgb(0x3C, 0x9E, 0x3C))
        } else {
            (Theme::XP_START_TOP, Theme::XP_START_BOT)
        };
        for row in 0..TASKBAR_H {
            let inset = if row < 4 {
                4 - row
            } else if row > TASKBAR_H - 5 {
                row - (TASKBAR_H - 5)
            } else {
                0
            };
            let w = (start_r.w - inset).max(0);
            let col = if row <= 1 {
                Color::rgb(0x6C, 0xCA, 0x6C)
            } else {
                Color::lerp(st_top, st_bot, row - 2, TASKBAR_H - 3)
            };
            canvas.fill_rect(Rect::new(0, tb_y + row, w, 1), col);
            if w > 0 {
                canvas.fill_rect(Rect::new(w - 1, tb_y + row, 1, 1), Color::rgb(0x1B, 0x5E, 0x1B));
            }
        }
        icons::draw_icon(canvas, 10, tb_y + 9, 16, IconId::StartFlag);
        font::draw_text_shadow(canvas, 34, tb_y + 9, "start", Color::WHITE, Color::rgb(20, 70, 20), 2, true);

        // 3. Quick Launch Icons
        for (i, &(_, icon)) in QUICK_LAUNCH.iter().enumerate() {
            let qx = 110 + (i as i32) * 28;
            icons::draw_icon(canvas, qx + 4, tb_y + 9, 16, icon);
        }
        canvas.fill_rect(Rect::new(196, tb_y + 6, 1, TASKBAR_H - 12), Color::rgb(0x16, 0x42, 0x9E));
        canvas.fill_rect(Rect::new(197, tb_y + 6, 1, TASKBAR_H - 12), Color::rgb(0x4D, 0x8E, 0xF0));

        // 4. System Tray on Right
        let tray_w = self.tray_width();
        let tray_x = (screen_w - tray_w).max(220);
        let tray_r = Rect::new(tray_x, tb_y, screen_w - tray_x, TASKBAR_H);
        canvas.fill_rect(Rect::new(tray_r.x, tb_y, tray_r.w, 2), Color::rgb(0x5C, 0xB6, 0xFF));
        canvas.fill_gradient_v(
            Rect::new(tray_r.x, tb_y + 2, tray_r.w, TASKBAR_H - 2),
            Theme::XP_TRAY_BLUE,
            Color::rgb(0x0B, 0x6F, 0xCC),
        );
        canvas.fill_rect(Rect::new(tray_r.x, tb_y, 1, TASKBAR_H), Color::rgb(0x09, 0x55, 0xA8));

        // Workspace switcher (1 2 3 4)
        let mut cx = tray_x + 8;
        for ws in 0..4u8 {
            let wr = Rect::new(cx, tb_y + 6, 20, 22);
            let active = scene.active_workspace == ws;
            let bg = if active { Color::rgb(0x08, 0x4C, 0x9E) } else { Color::rgb(0x2A, 0x9E, 0xFA) };
            canvas.fill_rect(wr, bg);
            canvas.draw_rect_outline(wr, 1, if active { Color::WHITE } else { Color::rgb(0x0A, 0x5E, 0xB8) });
            let lbl = format!("{}", ws + 1);
            font::draw_text(canvas, wr.x + 5, wr.y + 4, &lbl, Color::WHITE, 2);
            cx += 22;
        }

        // Layout toggle (Float / Tile)
        let lay_r = Rect::new(cx + 4, tb_y + 6, 60, 22);
        let lay_lbl = match scene.layout_mode {
            LayoutMode::Floating => "Float",
            LayoutMode::Tiled => "Tile",
        };
        canvas.fill_rect(lay_r, Color::rgb(0x16, 0x72, 0xD2));
        canvas.draw_rect_outline(lay_r, 1, Color::rgb(0x74, 0xC0, 0xFF));
        let lw = font::measure_text_width(lay_lbl, 2);
        font::draw_text(canvas, lay_r.x + (lay_r.w - lw) / 2, lay_r.y + 4, lay_lbl, Color::WHITE, 2);
        cx += 72;

        // Live RAM readout
        let stats = format!("RAM {}M/{}M", self.mem_mb, self.mem_total_mb);
        font::draw_text(canvas, cx, tb_y + 10, &stats, Color::WHITE, 2);

        // 5. Running Window Task Buttons
        let active_nodes: Vec<_> = scene
            .nodes
            .iter()
            .filter(|n| n.workspace == scene.active_workspace)
            .collect();
        if !active_nodes.is_empty() {
            let mut tx = 204;
            let avail_w = (tray_x - tx - 8).max(0);
            let btn_w = (avail_w / active_nodes.len() as i32 - 4).clamp(80, 176);
            for node in active_nodes {
                if tx + btn_w > tray_x - 4 {
                    break;
                }
                let br = Rect::new(tx, tb_y + 5, btn_w, TASKBAR_H - 10);
                let active = node.focused && !node.minimized;
                if active {
                    canvas.fill_gradient_v(br, Color::rgb(0x16, 0x3D, 0x98), Color::rgb(0x1E, 0x4E, 0xC0));
                    canvas.draw_rect_outline(br, 1, Color::rgb(0x0D, 0x2A, 0x70));
                    canvas.fill_rect(Rect::new(br.x + 1, br.bottom() - 2, br.w - 2, 1), Color::rgb(0x4E, 0x86, 0xEA));
                } else {
                    canvas.fill_gradient_v(br, Color::rgb(0x4C, 0x92, 0xF8), Color::rgb(0x2C, 0x6C, 0xDE));
                    canvas.draw_rect_outline(br, 1, Color::rgb(0x18, 0x48, 0xA8));
                    canvas.fill_rect(Rect::new(br.x + 1, br.y + 1, br.w - 2, 1), Color::rgb(0x7C, 0xB8, 0xFF));
                }
                icons::draw_icon(canvas, br.x + 5, br.y + 4, 16, IconId::from_title(&node.title));
                let max_chars = ((btn_w - 28).max(12) / (font::GLYPH_W * 2)) as usize;
                let lbl: String = node.title.chars().take(max_chars).collect();
                font::draw_text(canvas, br.x + 25, br.y + 5, &lbl, Color::WHITE, 2);
                tx += btn_w + 4;
            }
        }

        // Suppress unused warning if panel_r isn't used directly after this
        let _ = panel_r;

        // 6. Classic Two-Column XP Start Menu
        if self.launcher_open {
            let lr = self.launcher_rect(screen_h);
            let t = Theme::XP_LUNA;

            // Outer frame
            canvas.fill_rect(lr, Theme::XP_TITLE_ACTIVE_BORDER);

            // Top blue header bar
            let hdr = Rect::new(lr.x + 2, lr.y + 2, lr.w - 4, 42);
            canvas.fill_gradient_v(hdr, Theme::XP_TITLE_ACTIVE_TOP, Theme::XP_TITLE_ACTIVE_MID);
            icons::draw_icon(canvas, hdr.x + 10, hdr.y + 9, 24, IconId::MyComputer);
            font::draw_text_shadow(canvas, hdr.x + 42, hdr.y + 13, "PetraOS", Color::WHITE, Color::rgb(0, 28, 108), 2, true);

            // Left white column (All Apps)
            let left_col = Rect::new(lr.x + 2, hdr.bottom(), 196, lr.h - 42 - 34);
            canvas.fill_rect(left_col, Color::WHITE);
            for (i, app) in BUILTIN_APPS.iter().enumerate() {
                let item_r = Rect::new(left_col.x + 6, left_col.y + 6 + (i as i32) * 36, left_col.w - 12, 32);
                icons::draw_icon(canvas, item_r.x + 4, item_r.y + 4, 24, app.icon);
                font::draw_text(canvas, item_r.x + 34, item_r.y + 9, app.name, t.text_primary, 2);
            }

            // Right light-blue column (#D3E5FA) for System Places
            let right_col = Rect::new(left_col.right(), hdr.bottom(), lr.right() - 2 - left_col.right(), left_col.h);
            canvas.fill_rect(right_col, t.accent_soft);
            canvas.fill_rect(Rect::new(right_col.x, right_col.y, 1, right_col.h), Color::rgb(0x95, 0xBD, 0xEE));
            for (i, &(label, _, icon)) in START_PLACES.iter().enumerate() {
                let item_r = Rect::new(right_col.x + 6, right_col.y + 8 + (i as i32) * 34, right_col.w - 12, 30);
                icons::draw_icon(canvas, item_r.x + 4, item_r.y + 7, 16, icon);
                font::draw_text(canvas, item_r.x + 24, item_r.y + 8, label, Color::rgb(0x00, 0x26, 0x64), 2);
            }

            // Bottom blue footer bar
            let ftr = Rect::new(lr.x + 2, left_col.bottom(), lr.w - 4, lr.bottom() - 2 - left_col.bottom());
            canvas.fill_gradient_v(ftr, Theme::XP_TASKBAR_BLUE, Theme::XP_TASKBAR_BOT);
            font::draw_text(canvas, ftr.x + 12, ftr.y + 8, "PetraOS Luna", Color::rgb(210, 228, 255), 2);
        }
    }

    pub fn handle_click(
        &mut self,
        pos: Point,
        scene: &SceneGraph,
        screen_w: i32,
        screen_h: i32,
    ) -> PanelAction {
        // Start menu items
        if self.launcher_open {
            let lr = self.launcher_rect(screen_h);
            if lr.contains(pos) {
                let left_col_x = lr.x + 2;
                let left_col_w = 196;
                let body_y = lr.y + 44;
                for (i, app) in BUILTIN_APPS.iter().enumerate() {
                    let item_r = Rect::new(left_col_x + 6, body_y + 6 + (i as i32) * 36, left_col_w - 12, 32);
                    if item_r.contains(pos) {
                        self.launcher_open = false;
                        return PanelAction::LaunchApp(app.binary);
                    }
                }
                let right_col_x = left_col_x + left_col_w;
                let right_col_w = lr.right() - 2 - right_col_x;
                for (i, &(_, bin, _)) in START_PLACES.iter().enumerate() {
                    let item_r = Rect::new(right_col_x + 6, body_y + 8 + (i as i32) * 34, right_col_w - 12, 30);
                    if item_r.contains(pos) {
                        self.launcher_open = false;
                        return PanelAction::LaunchApp(bin);
                    }
                }
                return PanelAction::None;
            }
        }

        // Bottom taskbar
        let tb_y = screen_h - TASKBAR_H;
        if pos.y >= tb_y && pos.y < screen_h {
            if self.start_button_rect(screen_h).contains(pos) {
                return PanelAction::ToggleLauncher;
            }
            for (i, &(bin, _)) in QUICK_LAUNCH.iter().enumerate() {
                let qr = Rect::new(110 + (i as i32) * 28, tb_y + 5, 24, 24);
                if qr.contains(pos) {
                    return PanelAction::LaunchApp(bin);
                }
            }

            let tray_w = self.tray_width();
            let tray_x = (screen_w - tray_w).max(220);
            if pos.x >= tray_x {
                let mut cx = tray_x + 8;
                for ws in 0..4u8 {
                    let wr = Rect::new(cx, tb_y + 6, 20, 22);
                    if wr.contains(pos) {
                        return PanelAction::SwitchWorkspace(ws);
                    }
                    cx += 22;
                }
                let lay_r = Rect::new(cx + 4, tb_y + 6, 60, 22);
                if lay_r.contains(pos) {
                    return PanelAction::ToggleLayout;
                }
                return PanelAction::None;
            }

            let active_nodes: Vec<_> = scene
                .nodes
                .iter()
                .filter(|n| n.workspace == scene.active_workspace)
                .collect();
            if !active_nodes.is_empty() {
                let mut tx = 204;
                let avail_w = (tray_x - tx - 8).max(0);
                let btn_w = (avail_w / active_nodes.len() as i32 - 4).clamp(80, 176);
                for node in active_nodes {
                    if tx + btn_w > tray_x - 4 {
                        break;
                    }
                    let br = Rect::new(tx, tb_y + 5, btn_w, TASKBAR_H - 10);
                    if br.contains(pos) {
                        return PanelAction::FocusWindow(node.id);
                    }
                    tx += btn_w + 4;
                }
            }
        }

        PanelAction::None
    }
}
