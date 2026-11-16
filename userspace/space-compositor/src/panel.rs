//! XFCE-ergonomic + KDE Breeze-styled top panel, workspace pager, launcher, and bottom dock.

use crate::scene::{LayoutMode, SceneGraph};
use space_core::font;
use space_core::{Canvas, Color, Point, Rect};
use space_ui::Theme;

pub const TOP_PANEL_H: i32 = 34;
pub const DOCK_H: i32 = 44;

#[derive(Debug, Clone, Copy)]
pub struct AppEntry {
    pub name: &'static str,
    pub binary: &'static str,
    pub color: Color,
}

pub const BUILTIN_APPS: [AppEntry; 6] = [
    AppEntry {
        name: "Files",
        binary: "space-files",
        color: Color::rgb(61, 174, 233),
    },
    AppEntry {
        name: "Terminal",
        binary: "space-term",
        color: Color::rgb(46, 204, 113),
    },
    AppEntry {
        name: "Tasks",
        binary: "space-tasks",
        color: Color::rgb(230, 126, 34),
    },
    AppEntry {
        name: "Notepad",
        binary: "space-pad",
        color: Color::rgb(155, 89, 182),
    },
    AppEntry {
        name: "Calc",
        binary: "space-calc",
        color: Color::rgb(52, 152, 219),
    },
    AppEntry {
        name: "Control",
        binary: "space-control",
        color: Color::rgb(231, 76, 60),
    },
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

    pub fn work_area(&self, screen_w: i32, screen_h: i32) -> Rect {
        Rect::new(0, TOP_PANEL_H, screen_w, screen_h - TOP_PANEL_H - DOCK_H - 8)
    }

    pub fn dock_rect(&self, screen_w: i32, screen_h: i32) -> Rect {
        let w = (BUILTIN_APPS.len() as i32) * 96 + 16;
        let x = (screen_w - w) / 2;
        Rect::new(x, screen_h - DOCK_H - 6, w, DOCK_H)
    }

    pub fn launcher_rect(&self) -> Rect {
        Rect::new(8, TOP_PANEL_H + 4, 240, 260)
    }

    pub fn paint(&self, canvas: &mut dyn Canvas, scene: &SceneGraph, screen_w: i32, screen_h: i32) {
        let t = Theme::BREEZE_DARK;

        // 1. Top Panel
        let panel_r = Rect::new(0, 0, screen_w, TOP_PANEL_H);
        canvas.fill_rect(panel_r, t.bg_panel);
        canvas.fill_rect(Rect::new(0, TOP_PANEL_H - 1, screen_w, 1), t.border);

        // Launcher button
        let menu_r = Rect::new(6, 4, 84, 26);
        let menu_bg = if self.launcher_open { t.accent } else { t.bg_surface_alt };
        canvas.fill_rounded_rect(menu_r, 5, menu_bg);
        font::draw_text(canvas, menu_r.x + 10, menu_r.y + 6, "Petra", t.text_primary, 2);

        // Workspace pager (1..4)
        for ws in 0..4u8 {
            let wr = Rect::new(98 + (ws as i32) * 30, 4, 26, 26);
            let bg = if scene.active_workspace == ws { t.accent } else { t.bg_surface };
            canvas.fill_rounded_rect(wr, 4, bg);
            let label = format!("{}", ws + 1);
            font::draw_text(canvas, wr.x + 8, wr.y + 6, &label, t.text_primary, 2);
        }

        // Layout toggle pill (Float / Tile)
        let lay_r = Rect::new(226, 4, 72, 26);
        let lay_lbl = match scene.layout_mode {
            LayoutMode::Floating => "Float",
            LayoutMode::Tiled => "Tiled",
        };
        canvas.fill_rounded_rect(lay_r, 4, t.bg_surface_alt);
        font::draw_text(canvas, lay_r.x + 8, lay_r.y + 6, lay_lbl, t.accent, 2);

        // Taskbar window buttons
        let mut tx = 310;
        for node in &scene.nodes {
            if node.workspace != scene.active_workspace {
                continue;
            }
            let btn_w = 136;
            if tx + btn_w > screen_w - 220 {
                break;
            }
            let br = Rect::new(tx, 4, btn_w, 26);
            let bg = if node.focused && !node.minimized {
                t.bg_active
            } else {
                t.bg_surface
            };
            canvas.fill_rounded_rect(br, 4, bg);
            if node.focused {
                canvas.fill_rect(Rect::new(br.x + 4, br.bottom() - 2, br.w - 8, 2), t.accent);
            }
            let short: String = node.title.chars().take(10).collect();
            font::draw_text(canvas, br.x + 8, br.y + 6, &short, t.text_primary, 2);
            tx += btn_w + 6;
        }

        // System tray + RAM readout
        let stats = format!("RAM {}M/{}M", self.mem_mb, self.mem_total_mb);
        let sw = font::measure_text_width(&stats, 2);
        font::draw_text(canvas, screen_w - sw - 14, 10, &stats, t.text_secondary, 2);

        // 2. Bottom Quick-Launch Dock
        let dock_r = self.dock_rect(screen_w, screen_h);
        canvas.fill_rounded_rect(dock_r, 10, t.bg_panel);
        canvas.draw_rect_outline(dock_r, 1, t.border);
        for (i, app) in BUILTIN_APPS.iter().enumerate() {
            let bx = dock_r.x + 8 + (i as i32) * 96;
            let br = Rect::new(bx, dock_r.y + 6, 88, 32);
            canvas.fill_rounded_rect(br, 6, t.bg_surface_alt);
            canvas.fill_rect(Rect::new(br.x + 4, br.y + 4, 4, br.h - 8), app.color);
            font::draw_text(canvas, br.x + 12, br.y + 9, app.name, t.text_primary, 2);
        }

        // 3. Optional App Launcher Popup
        if self.launcher_open {
            let lr = self.launcher_rect();
            canvas.fill_rounded_rect(lr, 8, t.bg_surface);
            canvas.draw_rect_outline(lr, 1, t.accent);
            font::draw_text(canvas, lr.x + 12, lr.y + 10, "Applications", t.accent, 2);
            for (i, app) in BUILTIN_APPS.iter().enumerate() {
                let item_r = Rect::new(lr.x + 8, lr.y + 36 + (i as i32) * 35, lr.w - 16, 30);
                canvas.fill_rounded_rect(item_r, 5, t.bg_surface_alt);
                font::draw_text(canvas, item_r.x + 10, item_r.y + 8, app.name, t.text_primary, 2);
            }
        }
    }

    pub fn handle_click(
        &mut self,
        pos: Point,
        scene: &SceneGraph,
        screen_w: i32,
        screen_h: i32,
    ) -> PanelAction {
        // Launcher menu items
        if self.launcher_open {
            let lr = self.launcher_rect();
            if lr.contains(pos) {
                for (i, app) in BUILTIN_APPS.iter().enumerate() {
                    let item_r = Rect::new(lr.x + 8, lr.y + 36 + (i as i32) * 35, lr.w - 16, 30);
                    if item_r.contains(pos) {
                        self.launcher_open = false;
                        return PanelAction::LaunchApp(app.binary);
                    }
                }
                return PanelAction::None;
            }
        }

        // Top panel
        if pos.y < TOP_PANEL_H {
            if Rect::new(6, 4, 84, 26).contains(pos) {
                return PanelAction::ToggleLauncher;
            }
            for ws in 0..4u8 {
                let wr = Rect::new(98 + (ws as i32) * 30, 4, 26, 26);
                if wr.contains(pos) {
                    return PanelAction::SwitchWorkspace(ws);
                }
            }
            if Rect::new(226, 4, 72, 26).contains(pos) {
                return PanelAction::ToggleLayout;
            }
            let mut tx = 310;
            for node in &scene.nodes {
                if node.workspace != scene.active_workspace {
                    continue;
                }
                let br = Rect::new(tx, 4, 136, 26);
                if br.contains(pos) {
                    return PanelAction::FocusWindow(node.id);
                }
                tx += 142;
            }
        }

        // Bottom dock
        let dock_r = self.dock_rect(screen_w, screen_h);
        if dock_r.contains(pos) {
            for (i, app) in BUILTIN_APPS.iter().enumerate() {
                let bx = dock_r.x + 8 + (i as i32) * 96;
                let br = Rect::new(bx, dock_r.y + 6, 88, 32);
                if br.contains(pos) {
                    return PanelAction::LaunchApp(app.binary);
                }
            }
        }

        PanelAction::None
    }
}
