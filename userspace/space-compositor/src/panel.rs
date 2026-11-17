//! SerenityOS-inspired bottom taskbar, launcher menu, and tray applets styled in Petra Slate.

use crate::scene::{LayoutMode, SceneGraph};
use space_core::font;
use space_core::icons::{self, IconId};
use space_core::{Canvas, Color, Point, Rect};
use space_ui::Theme;
use std::time::{SystemTime, UNIX_EPOCH};

pub const TASKBAR_H: i32 = 32;
const MENU_W: i32 = 388;
const MENU_H: i32 = 388;
const MENU_HDR_H: i32 = 48;
const MENU_FTR_H: i32 = 36;
const LEFT_COL_W: i32 = 200;
const LAUNCHER_BTN_W: i32 = 88;
const SHOW_DESKTOP_W: i32 = 26;
const PAGER_W: i32 = 30;
const METERS_W: i32 = 52;
const LAYOUT_W: i32 = 46;
const CLOCK_W: i32 = 72;

#[derive(Debug, Clone)]
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
        icon: IconId::new("app-manager"),
    },
    AppEntry {
        name: "Files",
        subtitle: "Explorer++ File Browser",
        binary: "space-files",
        icon: IconId::new("files"),
    },
];

pub const BUILTIN_APPS: [AppEntry; 5] = [
    AppEntry {
        name: "Terminal",
        subtitle: "",
        binary: "space-term",
        icon: IconId::new("terminal"),
    },
    AppEntry {
        name: "Text Editor",
        subtitle: "",
        binary: "space-pad",
        icon: IconId::new("notepad"),
    },
    AppEntry {
        name: "Calculator",
        subtitle: "",
        binary: "space-calc",
        icon: IconId::new("calc"),
    },
    AppEntry {
        name: "Task Manager",
        subtitle: "",
        binary: "space-tasks",
        icon: IconId::new("tasks"),
    },
    AppEntry {
        name: "Control Panel",
        subtitle: "",
        binary: "space-control",
        icon: IconId::new("control-panel"),
    },
];

const QUICK_LAUNCH: [(&str, IconId); 4] = [
    ("space-term", IconId::new("terminal")),
    ("space-files", IconId::new("files")),
    ("space-apps", IconId::new("app-manager")),
    ("space-control", IconId::new("control-panel")),
];

const SYSTEM_PLACES: [(&str, &str, IconId, bool); 5] = [
    ("Computer", "space-files", IconId::new("my-computer"), true),
    ("Applications", "space-apps", IconId::new("app-manager"), true),
    ("Control Panel", "space-control", IconId::new("control-panel"), false),
    ("System Monitor", "space-tasks", IconId::new("tasks"), false),
    ("Run Command...", "space-term", IconId::new("terminal"), false),
];

pub enum PanelAction {
    None,
    ToggleLauncher,
    SwitchWorkspace(u8),
    ToggleLayout,
    FocusWindow(u32),
    LaunchApp(&'static str),
    ShowDesktop,
}

pub struct DesktopPanel {
    pub launcher_open: bool,
    pub mem_pct: u32,
    pub mem_mb: u32,
    pub mem_total_mb: u32,
    pub cpu_pct: u32,
    prev_cpu_total: u64,
    prev_cpu_idle: u64,
}

impl DesktopPanel {
    pub fn new() -> Self {
        Self {
            launcher_open: false,
            mem_pct: 0,
            mem_mb: 0,
            mem_total_mb: 0,
            cpu_pct: 0,
            prev_cpu_total: 0,
            prev_cpu_idle: 0,
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
                let used_kb = total_kb.saturating_sub(avail_kb);
                self.mem_mb = (used_kb / 1024) as u32;
                self.mem_pct = ((used_kb * 100) / total_kb).min(100) as u32;
            }
        }
        if let Ok(stat) = std::fs::read_to_string("/proc/stat") {
            if let Some(line) = stat.lines().next() {
                let vals: Vec<u64> = line
                    .split_whitespace()
                    .skip(1)
                    .filter_map(|s| s.parse::<u64>().ok())
                    .collect();
                if vals.len() >= 4 {
                    let idle = vals[3] + vals.get(4).copied().unwrap_or(0);
                    let total: u64 = vals.iter().sum();
                    if self.prev_cpu_total > 0 && total > self.prev_cpu_total {
                        let dt = total - self.prev_cpu_total;
                        let di = idle.saturating_sub(self.prev_cpu_idle);
                        self.cpu_pct = (((dt.saturating_sub(di)) * 100) / dt).min(100) as u32;
                    }
                    self.prev_cpu_total = total;
                    self.prev_cpu_idle = idle;
                }
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
        PAGER_W + METERS_W + LAYOUT_W + CLOCK_W + SHOW_DESKTOP_W + 26
    }

    fn launcher_button_rect(&self, screen_h: i32) -> Rect {
        Rect::new(4, screen_h - TASKBAR_H + 3, LAUNCHER_BTN_W, TASKBAR_H - 6)
    }

    fn quick_launch_start_x(&self) -> i32 {
        4 + LAUNCHER_BTN_W + 10
    }

    fn draw_bevel_box(canvas: &mut dyn Canvas, r: Rect, sunken: bool, top: Color, bot: Color) {
        canvas.fill_gradient_v(r, top, bot);
        let (tl, br) = if sunken {
            (Theme::PANEL_BORDER, Color::rgb(0x63, 0x99, 0xDB))
        } else {
            (Color::rgb(0x6C, 0xA2, 0xE3), Theme::PANEL_BORDER)
        };
        canvas.fill_rect(Rect::new(r.x, r.y, r.w, 1), tl);
        canvas.fill_rect(Rect::new(r.x, r.y, 1, r.h), tl);
        canvas.fill_rect(Rect::new(r.x, r.bottom() - 1, r.w, 1), br);
        canvas.fill_rect(Rect::new(r.right() - 1, r.y, 1, r.h), br);
    }

    fn draw_etched_divider(canvas: &mut dyn Canvas, x: i32, tb_y: i32) {
        canvas.fill_rect(Rect::new(x, tb_y + 5, 1, TASKBAR_H - 10), Theme::PANEL_BORDER);
        canvas.fill_rect(Rect::new(x + 1, tb_y + 5, 1, TASKBAR_H - 10), Theme::PANEL_TOP_HI);
    }

    fn clock_string() -> String {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let day_secs = secs % 86_400;
        let h = day_secs / 3600;
        let m = (day_secs % 3600) / 60;
        let s = day_secs % 60;
        format!("{:02}:{:02}:{:02}", h, m, s)
    }

    pub fn paint(&self, canvas: &mut dyn Canvas, scene: &SceneGraph, screen_w: i32, screen_h: i32) {
        let tb_y = screen_h - TASKBAR_H;

        // 1. SerenityOS-structured Taskbar in Petra Slate Cobalt Palette
        canvas.fill_rect(Rect::new(0, tb_y, screen_w, 1), Theme::PANEL_BORDER);
        canvas.fill_rect(Rect::new(0, tb_y + 1, screen_w, 1), Theme::PANEL_TOP_HI);
        canvas.fill_gradient_v(
            Rect::new(0, tb_y + 2, screen_w, TASKBAR_H - 2),
            Theme::PANEL_TOP,
            Theme::PANEL_BOT,
        );

        // 2. Start / Petra Launcher Button (SerenityOS raised/sunken bevel)
        let btn_r = self.launcher_button_rect(screen_h);
        if self.launcher_open {
            Self::draw_bevel_box(
                canvas,
                btn_r,
                true,
                Color::rgb(0x16, 0x35, 0x61),
                Color::rgb(0x21, 0x4D, 0x87),
            );
        } else {
            Self::draw_bevel_box(
                canvas,
                btn_r,
                false,
                Color::rgb(0x3D, 0x76, 0xBF),
                Color::rgb(0x23, 0x52, 0x8F),
            );
        }
        let press_off = i32::from(self.launcher_open);
        icons::draw_icon(canvas, btn_r.x + 6 + press_off, btn_r.y + 4 + press_off, 18, &IconId::new("petra-logo"));
        font::draw_text_shadow(
            canvas,
            btn_r.x + 29 + press_off,
            btn_r.y + 5 + press_off,
            "Petra",
            Color::WHITE,
            Color::rgb(0x0D, 0x22, 0x44),
            2,
            true,
        );

        let div1_x = btn_r.right() + 5;
        Self::draw_etched_divider(canvas, div1_x, tb_y);

        // 3. Quick Launch Icons (SerenityOS style)
        let ql_x = self.quick_launch_start_x();
        for (i, (_, icon)) in QUICK_LAUNCH.iter().enumerate() {
            let qx = ql_x + (i as i32) * 26;
            icons::draw_icon(canvas, qx + 2, tb_y + 6, 20, icon);
        }
        let div2_x = ql_x + QUICK_LAUNCH.len() as i32 * 26 + 4;
        Self::draw_etched_divider(canvas, div2_x, tb_y);

        // 4. Right Tray Area (SerenityOS Pager + CPU/RAM Monitors + Layout + Clock + Show Desktop)
        let tray_w = self.tray_width();
        let tray_x = (screen_w - tray_w).max(div2_x + 12);
        let tray_r = Rect::new(tray_x, tb_y + 3, screen_w - tray_x - 3, TASKBAR_H - 6);
        Self::draw_bevel_box(canvas, tray_r, true, Theme::TRAY_BOT, Theme::TRAY_TOP);

        // 4a. 2x2 Virtual Desktop Pager Grid (SerenityOS style)
        let mut cx = tray_r.x + 5;
        let pager_r = Rect::new(cx, tb_y + 5, PAGER_W, TASKBAR_H - 10);
        for ws in 0..4u8 {
            let col = (ws % 2) as i32;
            let row = (ws / 2) as i32;
            let qr = Rect::new(pager_r.x + col * 15, pager_r.y + row * 11, 14, 10);
            let active = scene.active_workspace == ws;
            let has_win = scene.nodes.iter().any(|n| n.workspace == ws && !n.minimized);
            let bg = if active {
                Color::rgb(0x4E, 0x8F, 0xDC)
            } else {
                Color::rgb(0x16, 0x32, 0x5A)
            };
            canvas.fill_rect(qr, bg);
            canvas.draw_rect_outline(
                qr,
                1,
                if active { Color::rgb(0xB8, 0xDB, 0xFF) } else { Theme::PANEL_BORDER },
            );
            if has_win {
                canvas.fill_rect(
                    Rect::new(qr.x + 3, qr.y + 3, 6, 4),
                    if active { Color::WHITE } else { Color::rgb(0x88, 0xBE, 0xF5) },
                );
            }
        }
        cx += PAGER_W + 6;

        // 4b. Live CPU & RAM Mini-Meter Boxes (SerenityOS SystemMonitor applet style)
        for (idx, (pct, bar_col)) in [
            (self.cpu_pct, Color::rgb(0x46, 0xD1, 0x7A)),
            (self.mem_pct, Color::rgb(0x52, 0xB8, 0xFF)),
        ]
        .iter()
        .enumerate()
        {
            let mr = Rect::new(cx + idx as i32 * 25, tb_y + 5, 22, TASKBAR_H - 10);
            canvas.fill_rect(mr, Color::rgb(0x0D, 0x16, 0x24));
            canvas.draw_rect_outline(mr, 1, Theme::PANEL_BORDER);
            let fill_h = (((mr.h - 4) as u32 * (*pct).max(8)) / 100) as i32;
            canvas.fill_rect(
                Rect::new(mr.x + 2, mr.bottom() - 2 - fill_h, mr.w - 4, fill_h),
                *bar_col,
            );
        }
        cx += METERS_W + 4;

        // 4c. Layout Mode Toggle Chip (Float / Tile)
        let lay_r = Rect::new(cx, tb_y + 5, LAYOUT_W, TASKBAR_H - 10);
        let lay_lbl = match scene.layout_mode {
            LayoutMode::Floating => "Float",
            LayoutMode::Tiled => "Tile",
        };
        Self::draw_bevel_box(
            canvas,
            lay_r,
            scene.layout_mode == LayoutMode::Tiled,
            Color::rgb(0x2D, 0x5D, 0x9C),
            Color::rgb(0x1E, 0x44, 0x76),
        );
        let lw = font::measure_text_width(lay_lbl, 1);
        font::draw_text(canvas, lay_r.x + (lay_r.w - lw) / 2, lay_r.y + 6, lay_lbl, Color::WHITE, 1);
        cx += LAYOUT_W + 5;

        // 4d. Sunken Digital Clock (HH:MM:SS)
        let clk_r = Rect::new(cx, tb_y + 5, CLOCK_W, TASKBAR_H - 10);
        canvas.fill_rect(clk_r, Color::rgb(0x16, 0x35, 0x61));
        canvas.draw_rect_outline(clk_r, 1, Theme::PANEL_BORDER);
        let clk_str = Self::clock_string();
        let cw = font::measure_text_width(&clk_str, 2);
        font::draw_text(
            canvas,
            clk_r.x + (clk_r.w - cw) / 2,
            clk_r.y + 3,
            &clk_str,
            Color::rgb(0xEE, 0xF5, 0xFF),
            2,
        );
        cx += CLOCK_W + 5;

        // 4e. Show Desktop Button (SerenityOS far-right taskbar icon)
        let sd_r = Rect::new(cx, tb_y + 5, SHOW_DESKTOP_W - 2, TASKBAR_H - 10);
        Self::draw_bevel_box(
            canvas,
            sd_r,
            false,
            Color::rgb(0x38, 0x6F, 0xB4),
            Color::rgb(0x23, 0x52, 0x8F),
        );
        icons::draw_icon(canvas, sd_r.x + 4, sd_r.y + 3, 16, &IconId::new("show-desktop"));

        // 5. Running Window Task Buttons (SerenityOS raised vs sunken bevels)
        let active_nodes: Vec<_> = scene
            .nodes
            .iter()
            .filter(|n| n.workspace == scene.active_workspace)
            .collect();
        if !active_nodes.is_empty() {
            let mut tx = div2_x + 6;
            let avail_w = (tray_x - tx - 6).max(0);
            let btn_w = (avail_w / active_nodes.len() as i32 - 4).clamp(84, 188);
            for node in active_nodes {
                if tx + btn_w > tray_x - 4 {
                    break;
                }
                let br = Rect::new(tx, tb_y + 3, btn_w, TASKBAR_H - 6);
                let active = node.focused && !node.minimized;
                if active {
                    Self::draw_bevel_box(
                        canvas,
                        br,
                        true,
                        Color::rgb(0x16, 0x35, 0x61),
                        Color::rgb(0x1E, 0x46, 0x7E),
                    );
                    canvas.fill_rect(
                        Rect::new(br.x + 1, br.bottom() - 2, br.w - 2, 1),
                        Color::rgb(0x6C, 0xAD, 0xF5),
                    );
                } else {
                    Self::draw_bevel_box(
                        canvas,
                        br,
                        false,
                        Color::rgb(0x38, 0x6F, 0xB4),
                        Color::rgb(0x27, 0x56, 0x94),
                    );
                }
                let p_off = i32::from(active);
                icons::draw_icon(
                    canvas,
                    br.x + 5 + p_off,
                    br.y + 4 + p_off,
                    16,
                    &IconId::from_title(&node.title),
                );
                let max_chars = ((btn_w - 30).max(10) / 8) as usize;
                let lbl: String = node.title.chars().take(max_chars).collect();
                let txt_col = if node.minimized {
                    Color::rgb(0xB8, 0xCC, 0xE6)
                } else {
                    Color::WHITE
                };
                if active {
                    font::draw_text_bold(canvas, br.x + 25 + p_off, br.y + 5 + p_off, &lbl, txt_col, 2);
                } else {
                    font::draw_text(canvas, br.x + 25, br.y + 5, &lbl, txt_col, 2);
                }
                tx += btn_w + 4;
            }
        }

        // 6. PetraOS Launcher Menu
        if self.launcher_open {
            let lr = self.launcher_rect(screen_h);
            let t = Theme::PETRA_SLATE;

            canvas.fill_rect(
                Rect::new(lr.x + 4, lr.y + 4, lr.w, lr.h),
                Color::argb(75, 0, 0, 0),
            );
            canvas.fill_rect(lr, t.bg_window);
            canvas.draw_rect_outline(lr, 1, Theme::TITLE_ACTIVE_BORDER);

            let hdr = Rect::new(lr.x + 1, lr.y + 1, lr.w - 2, MENU_HDR_H);
            canvas.fill_gradient_v(hdr, Theme::TITLE_ACTIVE_TOP, Theme::TITLE_ACTIVE_BOT);
            canvas.fill_rect(Rect::new(hdr.x, hdr.y, hdr.w, 1), Theme::TITLE_ACTIVE_HI);
            canvas.fill_rect(Rect::new(hdr.x, hdr.bottom() - 1, hdr.w, 1), Theme::TITLE_ACTIVE_BORDER);

            icons::draw_icon(canvas, hdr.x + 10, hdr.y + 8, 32, &IconId::new("petra-logo"));
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
            let left_col = Rect::new(lr.x + 1, hdr.bottom(), LEFT_COL_W, body_h);
            canvas.fill_rect(left_col, Color::WHITE);

            let mut ly = left_col.y + 8;
            for app in PINNED_APPS.iter() {
                icons::draw_icon(canvas, left_col.x + 10, ly + 4, 28, &app.icon);
                font::draw_text_bold(canvas, left_col.x + 46, ly + 2, app.name, t.text_primary, 2);
                font::draw_text(canvas, left_col.x + 46, ly + 19, app.subtitle, t.text_secondary, 1);
                ly += 42;
            }

            canvas.fill_rect(Rect::new(left_col.x + 10, ly + 2, left_col.w - 20, 1), t.bevel_shadow);
            ly += 10;

            for app in BUILTIN_APPS.iter() {
                icons::draw_icon(canvas, left_col.x + 12, ly + 4, 22, &app.icon);
                font::draw_text(canvas, left_col.x + 42, ly + 7, app.name, t.text_primary, 2);
                ly += 34;
            }

            let right_col = Rect::new(
                left_col.right(),
                hdr.bottom(),
                lr.right() - 1 - left_col.right(),
                body_h,
            );
            canvas.fill_rect(right_col, Color::rgb(0xEC, 0xF1, 0xF7));
            canvas.fill_rect(Rect::new(right_col.x, right_col.y, 1, right_col.h), t.bevel_shadow);

            let mut ry = right_col.y + 8;
            for (idx, (label, _, icon, is_bold)) in SYSTEM_PLACES.iter().enumerate() {
                if idx == 2 {
                    canvas.fill_rect(Rect::new(right_col.x + 10, ry + 2, right_col.w - 20, 1), t.bevel_shadow);
                    canvas.fill_rect(Rect::new(right_col.x + 10, ry + 3, right_col.w - 20, 1), Color::WHITE);
                    ry += 10;
                }
                icons::draw_icon(canvas, right_col.x + 10, ry + 4, 20, icon);
                if *is_bold {
                    font::draw_text_bold(canvas, right_col.x + 36, ry + 6, label, t.text_primary, 2);
                } else {
                    font::draw_text(canvas, right_col.x + 36, ry + 6, label, t.text_primary, 2);
                }
                ry += 34;
            }

            let ftr = Rect::new(lr.x + 1, left_col.bottom(), lr.w - 2, MENU_FTR_H);
            canvas.fill_rect(Rect::new(ftr.x, ftr.y, ftr.w, 1), t.bevel_shadow);
            canvas.fill_gradient_v(
                Rect::new(ftr.x, ftr.y + 1, ftr.w, ftr.h - 1),
                Color::rgb(0xEA, 0xEE, 0xF4),
                Color::rgb(0xDC, 0xE2, 0xEB),
            );
            font::draw_text(canvas, ftr.x + 12, ftr.y + 11, "Petra Slate", t.text_secondary, 1);

            let logoff_x = ftr.right() - 180;
            icons::draw_icon(canvas, logoff_x, ftr.y + 7, 20, &IconId::new("log-off"));
            font::draw_text(canvas, logoff_x + 24, ftr.y + 10, "Log off", t.text_primary, 2);

            let turnoff_x = ftr.right() - 94;
            icons::draw_icon(canvas, turnoff_x, ftr.y + 7, 20, &IconId::new("turn-off"));
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

        let tb_y = screen_h - TASKBAR_H;
        if pos.y >= tb_y && pos.y < screen_h {
            if self.launcher_button_rect(screen_h).contains(pos) {
                return PanelAction::ToggleLauncher;
            }
            let ql_x = self.quick_launch_start_x();
            for (i, &(bin, _)) in QUICK_LAUNCH.iter().enumerate() {
                let qr = Rect::new(ql_x + (i as i32) * 26, tb_y + 4, 24, 24);
                if qr.contains(pos) {
                    return PanelAction::LaunchApp(bin);
                }
            }

            let div2_x = ql_x + QUICK_LAUNCH.len() as i32 * 26 + 4;
            let tray_w = self.tray_width();
            let tray_x = (screen_w - tray_w).max(div2_x + 12);
            let tray_r = Rect::new(tray_x, tb_y + 3, screen_w - tray_x - 3, TASKBAR_H - 6);
            if pos.x >= tray_x {
                let mut cx = tray_r.x + 5;
                let pager_r = Rect::new(cx, tb_y + 5, PAGER_W, TASKBAR_H - 10);
                for ws in 0..4u8 {
                    let col = (ws % 2) as i32;
                    let row = (ws / 2) as i32;
                    let qr = Rect::new(pager_r.x + col * 15, pager_r.y + row * 11, 14, 10);
                    if qr.contains(pos) {
                        return PanelAction::SwitchWorkspace(ws);
                    }
                }
                cx += PAGER_W + 6 + METERS_W + 4;
                let lay_r = Rect::new(cx, tb_y + 5, LAYOUT_W, TASKBAR_H - 10);
                if lay_r.contains(pos) {
                    return PanelAction::ToggleLayout;
                }
                cx += LAYOUT_W + 5 + CLOCK_W + 5;
                let sd_r = Rect::new(cx, tb_y + 5, SHOW_DESKTOP_W - 2, TASKBAR_H - 10);
                if sd_r.contains(pos) {
                    return PanelAction::ShowDesktop;
                }
                return PanelAction::None;
            }

            let active_nodes: Vec<_> = scene
                .nodes
                .iter()
                .filter(|n| n.workspace == scene.active_workspace)
                .collect();
            if !active_nodes.is_empty() {
                let mut tx = div2_x + 6;
                let avail_w = (tray_x - tx - 6).max(0);
                let btn_w = (avail_w / active_nodes.len() as i32 - 4).clamp(84, 188);
                for node in active_nodes {
                    if tx + btn_w > tray_x - 4 {
                        break;
                    }
                    let br = Rect::new(tx, tb_y + 3, btn_w, TASKBAR_H - 6);
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
