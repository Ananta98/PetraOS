//! Classic Windows XP (Luna Blue) bottom taskbar, system tray, and two-column Start Menu.

use crate::scene::{LayoutMode, SceneGraph};
use space_core::font;
use space_core::icons::{self, IconId};
use space_core::{Canvas, Color, Point, Rect};
use space_ui::Theme;

pub const TASKBAR_H: i32 = 34;
const MENU_W: i32 = 392;
const MENU_H: i32 = 420;
const MENU_HDR_H: i32 = 54;
const MENU_FTR_H: i32 = 38;
const LEFT_COL_W: i32 = 198;

#[derive(Debug, Clone, Copy)]
pub struct AppEntry {
    pub name: &'static str,
    pub subtitle: &'static str,
    pub binary: &'static str,
    pub icon: IconId,
}

pub const PINNED_APPS: [AppEntry; 2] = [
    AppEntry {
        name: "Terminal",
        subtitle: "Command Prompt",
        binary: "space-term",
        icon: IconId::Terminal,
    },
    AppEntry {
        name: "File Explorer",
        subtitle: "Browse Dual-Pane Files",
        binary: "space-files",
        icon: IconId::Files,
    },
];

pub const BUILTIN_APPS: [AppEntry; 4] = [
    AppEntry {
        name: "Notepad",
        subtitle: "",
        binary: "space-pad",
        icon: IconId::Notepad,
    },
    AppEntry {
        name: "Calculator",
        subtitle: "",
        binary: "space-calc",
        icon: IconId::Calc,
    },
    AppEntry {
        name: "Task Manager",
        subtitle: "",
        binary: "space-tasks",
        icon: IconId::Tasks,
    },
    AppEntry {
        name: "Control Panel",
        subtitle: "",
        binary: "space-control",
        icon: IconId::ControlPanel,
    },
];

const QUICK_LAUNCH: [(&str, IconId); 4] = [
    ("space-files", IconId::Files),
    ("space-term", IconId::Terminal),
    ("space-pad", IconId::Notepad),
    ("space-control", IconId::ControlPanel),
];

const START_PLACES: [(&str, &str, IconId, bool); 6] = [
    ("My Documents", "space-files", IconId::Files, true),
    ("My Computer", "space-files", IconId::MyComputer, true),
    ("Control Panel", "space-control", IconId::ControlPanel, false),
    ("Task Manager", "space-tasks", IconId::Tasks, false),
    ("Calculator", "space-calc", IconId::Calc, false),
    ("Run Command...", "space-term", IconId::Terminal, false),
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
        Rect::new(0, (screen_h - TASKBAR_H - MENU_H).max(0), MENU_W, MENU_H)
    }

    fn tray_width(&self) -> i32 {
        let stats = format!("{}M / {}M", self.mem_mb, self.mem_total_mb);
        let sw = font::measure_text_width(&stats, 2);
        88 + 64 + sw + 34
    }

    fn start_button_rect(&self, screen_h: i32) -> Rect {
        Rect::new(0, screen_h - TASKBAR_H, 106, TASKBAR_H)
    }

    pub fn paint(&self, canvas: &mut dyn Canvas, scene: &SceneGraph, screen_w: i32, screen_h: i32) {
        let tb_y = screen_h - TASKBAR_H;

        // 1. Taskbar Luna Blue Glossy Gradient
        canvas.fill_rect(Rect::new(0, tb_y, screen_w, 1), Color::rgb(0x16, 0x45, 0xAE));
        canvas.fill_rect(Rect::new(0, tb_y + 1, screen_w, 2), Theme::XP_TASKBAR_TOP_HI);
        canvas.fill_gradient_v(
            Rect::new(0, tb_y + 3, screen_w, TASKBAR_H - 3),
            Theme::XP_TASKBAR_BLUE,
            Theme::XP_TASKBAR_BOT,
        );

        // 2. Glossy Green Rounded Windows XP Start Button
        let start_r = self.start_button_rect(screen_h);
        let (st_top, st_bot) = if self.launcher_open {
            (Color::rgb(0x26, 0x72, 0x26), Color::rgb(0x3C, 0x9E, 0x3C))
        } else {
            (Color::rgb(0x46, 0xB0, 0x46), Color::rgb(0x28, 0x7E, 0x28))
        };
        for row in 0..TASKBAR_H {
            let dy = (row - TASKBAR_H / 2).abs();
            let curve = (dy * dy) / 28;
            let w = (start_r.w - curve).max(0);
            let col = if row <= 2 {
                Color::rgb(0x82, 0xDC, 0x82)
            } else {
                Color::lerp(st_top, st_bot, row - 2, TASKBAR_H - 3)
            };
            canvas.fill_rect(Rect::new(0, tb_y + row, w, 1), col);
            if w > 1 {
                canvas.fill_rect(Rect::new(w - 2, tb_y + row, 1, 1), Color::rgb(0x5A, 0xC4, 0x5A));
                canvas.fill_rect(Rect::new(w - 1, tb_y + row, 1, 1), Color::rgb(0x17, 0x52, 0x17));
            }
        }
        icons::draw_icon(canvas, 10, tb_y + 6, 22, IconId::StartFlag);
        font::draw_text_shadow(
            canvas,
            37,
            tb_y + 8,
            "start",
            Color::WHITE,
            Color::rgb(16, 62, 16),
            2,
            true,
        );

        // 3. Quick Launch Icons (20x20)
        for (i, &(_, icon)) in QUICK_LAUNCH.iter().enumerate() {
            let qx = 112 + (i as i32) * 28;
            icons::draw_icon(canvas, qx + 2, tb_y + 7, 20, icon);
        }
        let div_x = 112 + QUICK_LAUNCH.len() as i32 * 28 + 4;
        canvas.fill_rect(Rect::new(div_x, tb_y + 6, 1, TASKBAR_H - 12), Color::rgb(0x14, 0x3D, 0x94));
        canvas.fill_rect(Rect::new(div_x + 1, tb_y + 6, 1, TASKBAR_H - 12), Color::rgb(0x52, 0x94, 0xF4));

        // 4. System Tray on Right
        let tray_w = self.tray_width();
        let tray_x = (screen_w - tray_w).max(div_x + 20);
        let tray_r = Rect::new(tray_x, tb_y, screen_w - tray_x, TASKBAR_H);
        canvas.fill_rect(Rect::new(tray_r.x, tb_y, tray_r.w, 2), Color::rgb(0x5C, 0xB6, 0xFF));
        canvas.fill_gradient_v(
            Rect::new(tray_r.x, tb_y + 2, tray_r.w, TASKBAR_H - 2),
            Theme::XP_TRAY_BLUE,
            Color::rgb(0x0B, 0x6F, 0xCC),
        );
        canvas.fill_rect(Rect::new(tray_r.x, tb_y, 1, TASKBAR_H), Color::rgb(0x09, 0x55, 0xA8));
        canvas.fill_rect(Rect::new(tray_r.x + 1, tb_y + 2, 1, TASKBAR_H - 2), Color::rgb(0x46, 0xA8, 0xFA));

        // Workspace switcher (1 2 3 4)
        let mut cx = tray_x + 8;
        for ws in 0..4u8 {
            let wr = Rect::new(cx, tb_y + 6, 20, 22);
            let active = scene.active_workspace == ws;
            let bg = if active { Color::rgb(0x08, 0x4C, 0x9E) } else { Color::rgb(0x24, 0x94, 0xF2) };
            canvas.fill_rect(wr, bg);
            canvas.draw_rect_outline(wr, 1, if active { Color::WHITE } else { Color::rgb(0x0A, 0x5E, 0xB8) });
            let lbl = format!("{}", ws + 1);
            let lw = font::measure_text_width(&lbl, 2);
            font::draw_text(canvas, wr.x + (wr.w - lw) / 2, wr.y + 3, &lbl, Color::WHITE, 2);
            cx += 22;
        }

        // Layout toggle (Float / Tile)
        let lay_r = Rect::new(cx + 4, tb_y + 6, 56, 22);
        let lay_lbl = match scene.layout_mode {
            LayoutMode::Floating => "Float",
            LayoutMode::Tiled => "Tile",
        };
        canvas.fill_rect(lay_r, Color::rgb(0x16, 0x72, 0xD2));
        canvas.draw_rect_outline(lay_r, 1, Color::rgb(0x74, 0xC0, 0xFF));
        let lw = font::measure_text_width(lay_lbl, 2);
        font::draw_text(canvas, lay_r.x + (lay_r.w - lw) / 2, lay_r.y + 3, lay_lbl, Color::WHITE, 2);
        cx += 66;

        // Live RAM readout
        let stats = format!("{}M / {}M", self.mem_mb, self.mem_total_mb);
        font::draw_text(canvas, cx, tb_y + 9, &stats, Color::WHITE, 2);

        // 5. Running Window Task Buttons
        let active_nodes: Vec<_> = scene
            .nodes
            .iter()
            .filter(|n| n.workspace == scene.active_workspace)
            .collect();
        if !active_nodes.is_empty() {
            let mut tx = div_x + 8;
            let avail_w = (tray_x - tx - 8).max(0);
            let btn_w = (avail_w / active_nodes.len() as i32 - 4).clamp(90, 184);
            for node in active_nodes {
                if tx + btn_w > tray_x - 4 {
                    break;
                }
                let br = Rect::new(tx, tb_y + 4, btn_w, TASKBAR_H - 8);
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
                icons::draw_icon(canvas, br.x + 6, br.y + 4, 18, IconId::from_title(&node.title));
                let max_chars = ((btn_w - 32).max(12) / 8) as usize;
                let lbl: String = node.title.chars().take(max_chars).collect();
                font::draw_text(canvas, br.x + 28, br.y + 5, &lbl, Color::WHITE, 2);
                tx += btn_w + 4;
            }
        }

        // 6. Authentic Two-Column Windows XP Luna Start Menu
        if self.launcher_open {
            let lr = self.launcher_rect(screen_h);
            let t = Theme::XP_LUNA;

            // Drop shadow behind Start Menu
            canvas.fill_rect(
                Rect::new(lr.x + 4, lr.y + 4, lr.w, lr.h),
                Color::argb(85, 0, 0, 0),
            );

            // Outer frame
            canvas.fill_rect(lr, Theme::XP_TITLE_ACTIVE_BORDER);

            // Top glossy blue header bar with user avatar frame & orange highlight separator
            let hdr = Rect::new(lr.x + 2, lr.y + 2, lr.w - 4, MENU_HDR_H);
            canvas.fill_gradient_v(
                Rect::new(hdr.x, hdr.y, hdr.w, 16),
                Color::rgb(0x52, 0x9C, 0xFF),
                Color::rgb(0x16, 0x5E, 0xDB),
            );
            canvas.fill_gradient_v(
                Rect::new(hdr.x, hdr.y + 16, hdr.w, hdr.h - 18),
                Color::rgb(0x16, 0x5E, 0xDB),
                Color::rgb(0x2A, 0x76, 0xEC),
            );
            // Iconic XP orange-gold accent strip below the Start Menu header
            canvas.fill_rect(
                Rect::new(hdr.x, hdr.bottom() - 2, hdr.w, 2),
                Color::rgb(0xF6, 0x96, 0x28),
            );

            // Framed 40x40 user avatar badge on the left
            let av = Rect::new(hdr.x + 8, hdr.y + 6, 40, 40);
            canvas.fill_rounded_rect(av, 4, Color::WHITE);
            let av_in = Rect::new(av.x + 2, av.y + 2, av.w - 4, av.h - 4);
            canvas.fill_gradient_v(av_in, Color::rgb(0x74, 0xC2, 0xFA), Color::rgb(0x52, 0xA8, 0x3E));
            icons::draw_icon(canvas, av_in.x + 2, av_in.y + 2, 32, IconId::MyComputer);
            font::draw_text_shadow(
                canvas,
                hdr.x + 56,
                hdr.y + 17,
                "PetraOS User",
                Color::WHITE,
                Color::rgb(0, 28, 108),
                2,
                true,
            );

            let body_h = lr.h - MENU_HDR_H - MENU_FTR_H - 4;

            // Left white column (Pinned Apps + All Programs)
            let left_col = Rect::new(lr.x + 2, hdr.bottom(), LEFT_COL_W, body_h);
            canvas.fill_rect(left_col, Color::WHITE);

            let mut ly = left_col.y + 8;
            for app in PINNED_APPS.iter() {
                icons::draw_icon(canvas, left_col.x + 10, ly + 2, 32, app.icon);
                font::draw_text_bold(canvas, left_col.x + 48, ly + 2, app.name, t.text_primary, 2);
                font::draw_text(canvas, left_col.x + 48, ly + 19, app.subtitle, t.text_secondary, 1);
                ly += 42;
            }

            // Etched horizontal separator
            canvas.fill_rect(Rect::new(left_col.x + 10, ly + 2, left_col.w - 20, 1), Color::rgb(0xD6, 0xD3, 0xC6));
            ly += 10;

            for app in BUILTIN_APPS.iter() {
                icons::draw_icon(canvas, left_col.x + 12, ly + 3, 24, app.icon);
                font::draw_text(canvas, left_col.x + 44, ly + 7, app.name, t.text_primary, 2);
                ly += 34;
            }

            // Bottom "All Programs ▶" bar in left column
            let all_y = left_col.bottom() - 34;
            canvas.fill_rect(Rect::new(left_col.x + 10, all_y - 4, left_col.w - 20, 1), Color::rgb(0xD6, 0xD3, 0xC6));
            font::draw_text_bold(canvas, left_col.x + 38, all_y + 6, "All Programs", t.text_primary, 2);
            // Green XP arrow triangle
            let ax = left_col.right() - 28;
            let ay = all_y + 7;
            for r in 0..13i32 {
                let half = (6 - (r - 6).abs()).max(0);
                canvas.fill_rect(Rect::new(ax, ay + r, half + 2, 1), Color::rgb(0x38, 0x9E, 0x34));
            }

            // Right light-blue column (#D3E5FA) for System Places
            let right_col = Rect::new(
                left_col.right(),
                hdr.bottom(),
                lr.right() - 2 - left_col.right(),
                body_h,
            );
            canvas.fill_rect(right_col, t.accent_soft);
            canvas.fill_rect(Rect::new(right_col.x, right_col.y, 1, right_col.h), Color::rgb(0x95, 0xBD, 0xEE));

            let mut ry = right_col.y + 8;
            for (idx, &(label, _, icon, is_bold)) in START_PLACES.iter().enumerate() {
                if idx == 2 {
                    canvas.fill_rect(Rect::new(right_col.x + 10, ry + 2, right_col.w - 20, 1), Color::rgb(0xA6, 0xC6, 0xEE));
                    canvas.fill_rect(Rect::new(right_col.x + 10, ry + 3, right_col.w - 20, 1), Color::rgb(0xEA, 0xF3, 0xFF));
                    ry += 10;
                }
                icons::draw_icon(canvas, right_col.x + 10, ry + 3, 22, icon);
                let text_col = Color::rgb(0x0A, 0x24, 0x6A);
                if is_bold {
                    font::draw_text_bold(canvas, right_col.x + 38, ry + 6, label, text_col, 2);
                } else {
                    font::draw_text(canvas, right_col.x + 38, ry + 6, label, text_col, 2);
                }
                ry += 34;
            }

            // Bottom royal-blue footer bar with Log Off & Turn Off buttons
            let ftr = Rect::new(lr.x + 2, left_col.bottom(), lr.w - 4, MENU_FTR_H);
            canvas.fill_rect(Rect::new(ftr.x, ftr.y, ftr.w, 1), Color::rgb(0x3D, 0x80, 0xF0));
            canvas.fill_gradient_v(
                Rect::new(ftr.x, ftr.y + 1, ftr.w, ftr.h - 1),
                Color::rgb(0x28, 0x66, 0xE2),
                Color::rgb(0x12, 0x3D, 0x9C),
            );

            // Log off & Turn off badges on right side of footer
            let logoff_x = ftr.right() - 188;
            icons::draw_icon(canvas, logoff_x, ftr.y + 8, 22, IconId::LogOff);
            font::draw_text(canvas, logoff_x + 26, ftr.y + 11, "Log off", Color::WHITE, 2);

            let turnoff_x = ftr.right() - 96;
            icons::draw_icon(canvas, turnoff_x, ftr.y + 8, 22, IconId::TurnOff);
            font::draw_text(canvas, turnoff_x + 26, ftr.y + 11, "Turn off", Color::WHITE, 2);
        }
    }

    pub fn handle_click(
        &mut self,
        pos: Point,
        scene: &SceneGraph,
        screen_w: i32,
        screen_h: i32,
    ) -> PanelAction {
        if self.launcher_open {
            let lr = self.launcher_rect(screen_h);
            if lr.contains(pos) {
                let left_col_x = lr.x + 2;
                let body_y = lr.y + 2 + MENU_HDR_H;

                let mut ly = body_y + 8;
                for app in PINNED_APPS.iter() {
                    let r = Rect::new(left_col_x + 6, ly, LEFT_COL_W - 12, 40);
                    if r.contains(pos) {
                        self.launcher_open = false;
                        return PanelAction::LaunchApp(app.binary);
                    }
                    ly += 42;
                }
                ly += 10;
                for app in BUILTIN_APPS.iter() {
                    let r = Rect::new(left_col_x + 6, ly, LEFT_COL_W - 12, 32);
                    if r.contains(pos) {
                        self.launcher_open = false;
                        return PanelAction::LaunchApp(app.binary);
                    }
                    ly += 34;
                }

                let right_col_x = left_col_x + LEFT_COL_W;
                let right_col_w = lr.right() - 2 - right_col_x;
                let mut ry = body_y + 8;
                for (idx, &(_, bin, _, _)) in START_PLACES.iter().enumerate() {
                    if idx == 2 {
                        ry += 10;
                    }
                    let r = Rect::new(right_col_x + 6, ry, right_col_w - 12, 32);
                    if r.contains(pos) {
                        self.launcher_open = false;
                        return PanelAction::LaunchApp(bin);
                    }
                    ry += 34;
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
                let qr = Rect::new(112 + (i as i32) * 28, tb_y + 5, 24, 24);
                if qr.contains(pos) {
                    return PanelAction::LaunchApp(bin);
                }
            }

            let div_x = 112 + QUICK_LAUNCH.len() as i32 * 28 + 4;
            let tray_w = self.tray_width();
            let tray_x = (screen_w - tray_w).max(div_x + 20);
            if pos.x >= tray_x {
                let mut cx = tray_x + 8;
                for ws in 0..4u8 {
                    let wr = Rect::new(cx, tb_y + 6, 20, 22);
                    if wr.contains(pos) {
                        return PanelAction::SwitchWorkspace(ws);
                    }
                    cx += 22;
                }
                let lay_r = Rect::new(cx + 4, tb_y + 6, 56, 22);
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
                let mut tx = div_x + 8;
                let avail_w = (tray_x - tx - 8).max(0);
                let btn_w = (avail_w / active_nodes.len() as i32 - 4).clamp(90, 184);
                for node in active_nodes {
                    if tx + btn_w > tray_x - 4 {
                        break;
                    }
                    let br = Rect::new(tx, tb_y + 4, btn_w, TASKBAR_H - 8);
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
