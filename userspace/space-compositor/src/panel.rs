//! Petra Slate bottom taskbar, system tray, and OS launcher menu.

use crate::scene::{LayoutMode, SceneGraph};
use space_core::font;
use space_core::icons::{self, IconId};
use space_core::{Canvas, Color, Point, Rect};
use space_ui::Theme;

pub const TASKBAR_H: i32 = 34;
const MENU_W: i32 = 388;
const MENU_H: i32 = 388;
const MENU_HDR_H: i32 = 48;
const MENU_FTR_H: i32 = 36;
const LEFT_COL_W: i32 = 200;
const LAUNCHER_BTN_W: i32 = 96;

#[derive(Debug, Clone, Copy)]
pub struct AppEntry {
    pub name: &'static str,
    pub subtitle: &'static str,
    pub binary: &'static str,
    pub icon: IconId,
}

pub const PINNED_APPS: [AppEntry; 2] = [
    AppEntry {
        name: "Applications",
        subtitle: "Install & Manage Software",
        binary: "space-apps",
        icon: IconId::AppManager,
    },
    AppEntry {
        name: "Files",
        subtitle: "Dual-Pane File Explorer",
        binary: "space-files",
        icon: IconId::Files,
    },
];

pub const BUILTIN_APPS: [AppEntry; 5] = [
    AppEntry {
        name: "Terminal",
        subtitle: "",
        binary: "space-term",
        icon: IconId::Terminal,
    },
    AppEntry {
        name: "Text Editor",
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
    ("space-apps", IconId::AppManager),
    ("space-term", IconId::Terminal),
    ("space-control", IconId::ControlPanel),
];

const SYSTEM_PLACES: [(&str, &str, IconId, bool); 5] = [
    ("Computer", "space-files", IconId::MyComputer, true),
    ("Applications", "space-apps", IconId::AppManager, true),
    ("Control Panel", "space-control", IconId::ControlPanel, false),
    ("System Monitor", "space-tasks", IconId::Tasks, false),
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
        Rect::new(4, (screen_h - TASKBAR_H - MENU_H - 2).max(0), MENU_W, MENU_H)
    }

    fn tray_width(&self) -> i32 {
        let stats = format!("{}M / {}M", self.mem_mb, self.mem_total_mb);
        let sw = font::measure_text_width(&stats, 2);
        88 + 64 + sw + 34
    }

    fn launcher_button_rect(&self, screen_h: i32) -> Rect {
        Rect::new(4, screen_h - TASKBAR_H + 3, LAUNCHER_BTN_W, TASKBAR_H - 6)
    }

    fn quick_launch_start_x(&self) -> i32 {
        4 + LAUNCHER_BTN_W + 8
    }

    pub fn paint(&self, canvas: &mut dyn Canvas, scene: &SceneGraph, screen_w: i32, screen_h: i32) {
        let tb_y = screen_h - TASKBAR_H;

        // 1. Clean ReactOS-inspired Slate-Cobalt Taskbar
        canvas.fill_rect(Rect::new(0, tb_y, screen_w, 1), Theme::PANEL_BORDER);
        canvas.fill_rect(Rect::new(0, tb_y + 1, screen_w, 1), Theme::PANEL_TOP_HI);
        canvas.fill_gradient_v(
            Rect::new(0, tb_y + 2, screen_w, TASKBAR_H - 2),
            Theme::PANEL_TOP,
            Theme::PANEL_BOT,
        );

        // 2. Original PetraOS Launcher Button (no Windows flag / no "start")
        let btn_r = self.launcher_button_rect(screen_h);
        let (btn_top, btn_bot) = if self.launcher_open {
            (Color::rgb(0x16, 0x38, 0x66), Color::rgb(0x24, 0x52, 0x90))
        } else {
            (Color::rgb(0x3D, 0x76, 0xBF), Color::rgb(0x23, 0x52, 0x8F))
        };
        canvas.fill_rounded_rect(btn_r, 3, btn_bot);
        canvas.fill_gradient_v(
            Rect::new(btn_r.x + 1, btn_r.y + 1, btn_r.w - 2, btn_r.h - 2),
            btn_top,
            btn_bot,
        );
        canvas.draw_rect_outline(btn_r, 1, Theme::PANEL_BORDER);
        if !self.launcher_open {
            canvas.fill_rect(
                Rect::new(btn_r.x + 1, btn_r.y + 1, btn_r.w - 2, 1),
                Color::rgb(0x73, 0xA6, 0xE6),
            );
        }
        icons::draw_icon(canvas, btn_r.x + 7, btn_r.y + 4, 20, IconId::PetraLogo);
        font::draw_text_shadow(
            canvas,
            btn_r.x + 32,
            btn_r.y + 6,
            "Petra",
            Color::WHITE,
            Color::rgb(0x0D, 0x22, 0x44),
            2,
            true,
        );

        // 3. Quick Launch Icons (20x20)
        let ql_x = self.quick_launch_start_x();
        for (i, &(_, icon)) in QUICK_LAUNCH.iter().enumerate() {
            let qx = ql_x + (i as i32) * 28;
            icons::draw_icon(canvas, qx + 2, tb_y + 7, 20, icon);
        }
        let div_x = ql_x + QUICK_LAUNCH.len() as i32 * 28 + 4;
        canvas.fill_rect(Rect::new(div_x, tb_y + 6, 1, TASKBAR_H - 12), Theme::PANEL_BORDER);
        canvas.fill_rect(Rect::new(div_x + 1, tb_y + 6, 1, TASKBAR_H - 12), Theme::PANEL_TOP_HI);

        // 4. System Tray on Right
        let tray_w = self.tray_width();
        let tray_x = (screen_w - tray_w).max(div_x + 20);
        let tray_r = Rect::new(tray_x, tb_y + 2, screen_w - tray_x, TASKBAR_H - 2);
        canvas.fill_gradient_v(tray_r, Theme::TRAY_TOP, Theme::TRAY_BOT);
        canvas.fill_rect(Rect::new(tray_r.x, tb_y + 1, 1, TASKBAR_H - 1), Theme::PANEL_BORDER);
        canvas.fill_rect(Rect::new(tray_r.x + 1, tb_y + 2, 1, TASKBAR_H - 2), Theme::PANEL_TOP_HI);

        // Workspace switcher (1 2 3 4)
        let mut cx = tray_x + 8;
        for ws in 0..4u8 {
            let wr = Rect::new(cx, tb_y + 6, 20, 22);
            let active = scene.active_workspace == ws;
            let bg = if active {
                Color::rgb(0x36, 0x71, 0xB8)
            } else {
                Color::rgb(0x1E, 0x44, 0x76)
            };
            canvas.fill_rect(wr, bg);
            canvas.draw_rect_outline(
                wr,
                1,
                if active {
                    Color::rgb(0x88, 0xBE, 0xF5)
                } else {
                    Theme::PANEL_BORDER
                },
            );
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
        canvas.fill_rect(lay_r, Color::rgb(0x1E, 0x44, 0x76));
        canvas.draw_rect_outline(lay_r, 1, Color::rgb(0x4F, 0x84, 0xC6));
        let lw = font::measure_text_width(lay_lbl, 2);
        font::draw_text(canvas, lay_r.x + (lay_r.w - lw) / 2, lay_r.y + 3, lay_lbl, Color::WHITE, 2);
        cx += 66;

        // Live RAM readout
        let stats = format!("{}M / {}M", self.mem_mb, self.mem_total_mb);
        font::draw_text(canvas, cx, tb_y + 9, &stats, Color::rgb(0xE8, 0xF1, 0xFA), 2);

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
                    canvas.fill_gradient_v(br, Color::rgb(0x16, 0x35, 0x61), Color::rgb(0x1E, 0x46, 0x7E));
                    canvas.draw_rect_outline(br, 1, Theme::PANEL_BORDER);
                    canvas.fill_rect(
                        Rect::new(br.x + 1, br.bottom() - 2, br.w - 2, 1),
                        Color::rgb(0x6C, 0xAD, 0xF5),
                    );
                } else {
                    canvas.fill_gradient_v(br, Color::rgb(0x38, 0x6F, 0xB4), Color::rgb(0x27, 0x56, 0x94));
                    canvas.draw_rect_outline(br, 1, Theme::PANEL_BORDER);
                    canvas.fill_rect(
                        Rect::new(br.x + 1, br.y + 1, br.w - 2, 1),
                        Color::rgb(0x5E, 0x95, 0xD6),
                    );
                }
                icons::draw_icon(canvas, br.x + 6, br.y + 4, 18, IconId::from_title(&node.title));
                let max_chars = ((btn_w - 32).max(12) / 8) as usize;
                let lbl: String = node.title.chars().take(max_chars).collect();
                font::draw_text(canvas, br.x + 28, br.y + 5, &lbl, Color::WHITE, 2);
                tx += btn_w + 4;
            }
        }

        // 6. PetraOS Categorized Launcher Menu (ReactOS + SerenityOS structure)
        if self.launcher_open {
            let lr = self.launcher_rect(screen_h);
            let t = Theme::PETRA_SLATE;

            // Drop shadow
            canvas.fill_rect(
                Rect::new(lr.x + 4, lr.y + 4, lr.w, lr.h),
                Color::argb(75, 0, 0, 0),
            );
            canvas.fill_rect(lr, t.bg_window);
            canvas.draw_rect_outline(lr, 1, Theme::TITLE_ACTIVE_BORDER);

            // Top Slate-Cobalt Header Strip with PetraOS Logo
            let hdr = Rect::new(lr.x + 1, lr.y + 1, lr.w - 2, MENU_HDR_H);
            canvas.fill_gradient_v(hdr, Theme::TITLE_ACTIVE_TOP, Theme::TITLE_ACTIVE_BOT);
            canvas.fill_rect(Rect::new(hdr.x, hdr.y, hdr.w, 1), Theme::TITLE_ACTIVE_HI);
            canvas.fill_rect(Rect::new(hdr.x, hdr.bottom() - 1, hdr.w, 1), Theme::TITLE_ACTIVE_BORDER);

            icons::draw_icon(canvas, hdr.x + 10, hdr.y + 8, 32, IconId::PetraLogo);
            font::draw_text_shadow(
                canvas,
                hdr.x + 50,
                hdr.y + 9,
                "PetraOS",
                Color::WHITE,
                Color::rgb(0x10, 0x26, 0x4C),
                2,
                true,
            );
            font::draw_text(
                canvas,
                hdr.x + 50,
                hdr.y + 27,
                "PetraSpace Desktop",
                Color::rgb(0xC8, 0xDF, 0xFA),
                1,
            );

            let body_h = lr.h - MENU_HDR_H - MENU_FTR_H - 2;

            // Left white column: Applications
            let left_col = Rect::new(lr.x + 1, hdr.bottom(), LEFT_COL_W, body_h);
            canvas.fill_rect(left_col, Color::WHITE);

            let mut ly = left_col.y + 8;
            for app in PINNED_APPS.iter() {
                icons::draw_icon(canvas, left_col.x + 10, ly + 4, 28, app.icon);
                font::draw_text_bold(canvas, left_col.x + 46, ly + 2, app.name, t.text_primary, 2);
                font::draw_text(canvas, left_col.x + 46, ly + 19, app.subtitle, t.text_secondary, 1);
                ly += 42;
            }

            canvas.fill_rect(Rect::new(left_col.x + 10, ly + 2, left_col.w - 20, 1), t.bevel_shadow);
            ly += 10;

            for app in BUILTIN_APPS.iter() {
                icons::draw_icon(canvas, left_col.x + 12, ly + 4, 22, app.icon);
                font::draw_text(canvas, left_col.x + 42, ly + 7, app.name, t.text_primary, 2);
                ly += 34;
            }

            // Right cool-slate column: System Places
            let right_col = Rect::new(
                left_col.right(),
                hdr.bottom(),
                lr.right() - 1 - left_col.right(),
                body_h,
            );
            canvas.fill_rect(right_col, Color::rgb(0xEC, 0xF1, 0xF7));
            canvas.fill_rect(Rect::new(right_col.x, right_col.y, 1, right_col.h), t.bevel_shadow);

            let mut ry = right_col.y + 8;
            for (idx, &(label, _, icon, is_bold)) in SYSTEM_PLACES.iter().enumerate() {
                if idx == 2 {
                    canvas.fill_rect(Rect::new(right_col.x + 10, ry + 2, right_col.w - 20, 1), t.bevel_shadow);
                    canvas.fill_rect(Rect::new(right_col.x + 10, ry + 3, right_col.w - 20, 1), Color::WHITE);
                    ry += 10;
                }
                icons::draw_icon(canvas, right_col.x + 10, ry + 4, 20, icon);
                if is_bold {
                    font::draw_text_bold(canvas, right_col.x + 36, ry + 6, label, t.text_primary, 2);
                } else {
                    font::draw_text(canvas, right_col.x + 36, ry + 6, label, t.text_primary, 2);
                }
                ry += 34;
            }

            // Clean neutral footer strip
            let ftr = Rect::new(lr.x + 1, left_col.bottom(), lr.w - 2, MENU_FTR_H);
            canvas.fill_rect(Rect::new(ftr.x, ftr.y, ftr.w, 1), t.bevel_shadow);
            canvas.fill_gradient_v(
                Rect::new(ftr.x, ftr.y + 1, ftr.w, ftr.h - 1),
                Color::rgb(0xEA, 0xEE, 0xF4),
                Color::rgb(0xDC, 0xE2, 0xEB),
            );
            font::draw_text(canvas, ftr.x + 12, ftr.y + 11, "Petra Slate", t.text_secondary, 1);

            let logoff_x = ftr.right() - 180;
            icons::draw_icon(canvas, logoff_x, ftr.y + 7, 20, IconId::LogOff);
            font::draw_text(canvas, logoff_x + 24, ftr.y + 10, "Log off", t.text_primary, 2);

            let turnoff_x = ftr.right() - 94;
            icons::draw_icon(canvas, turnoff_x, ftr.y + 7, 20, IconId::TurnOff);
            font::draw_text(canvas, turnoff_x + 24, ftr.y + 10, "Shut down", t.text_primary, 2);
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
                let left_col_x = lr.x + 1;
                let body_y = lr.y + 1 + MENU_HDR_H;

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
                let right_col_w = lr.right() - 1 - right_col_x;
                let mut ry = body_y + 8;
                for (idx, &(_, bin, _, _)) in SYSTEM_PLACES.iter().enumerate() {
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
            if self.launcher_button_rect(screen_h).contains(pos) {
                return PanelAction::ToggleLauncher;
            }
            let ql_x = self.quick_launch_start_x();
            for (i, &(bin, _)) in QUICK_LAUNCH.iter().enumerate() {
                let qr = Rect::new(ql_x + (i as i32) * 28, tb_y + 5, 24, 24);
                if qr.contains(pos) {
                    return PanelAction::LaunchApp(bin);
                }
            }

            let div_x = ql_x + QUICK_LAUNCH.len() as i32 * 28 + 4;
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
