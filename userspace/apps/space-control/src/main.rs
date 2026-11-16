use space_core::font;
use space_core::icons::{self, IconId};
use space_core::void_pkg::{PackageInfo, RunitManager, ServiceInfo, XbpsClient};
use space_core::{Canvas, Color, Rect};
use space_ui::{
    draw_panel_3d, draw_progress_bar, list_item_at, AppLogic, Button, EventListener, Lifecycle,
    Paintable, SpaceClient, Theme, UiEvent,
};
use std::fs;
use std::process::Command;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ControlTab {
    Display,
    Audio,
    Runit,
    Xbps,
}

const RES_MODES: [&str; 5] = [
    "1024x768",
    "1280x800",
    "1440x900",
    "1600x900",
    "1920x1080",
];

struct ControlApp {
    width: u32,
    height: u32,
    tab: ControlTab,
    btn_disp: Button,
    btn_audio: Button,
    btn_runit: Button,
    btn_xbps: Button,
    // Display
    drm_info: Vec<String>,
    active_res: String,
    res_buttons: [Button; 5],
    // Audio
    volume: i32,
    muted: bool,
    btn_vol_down: Button,
    btn_vol_up: Button,
    btn_mute: Button,
    // Runit
    services: Vec<ServiceInfo>,
    sel_svc: Option<usize>,
    btn_svc_up: Button,
    btn_svc_down: Button,
    btn_svc_restart: Button,
    // XBPS
    packages: Vec<PackageInfo>,
    pkg_scroll: usize,
    status: String,
}

impl ControlApp {
    fn new(width: u32, height: u32) -> Self {
        let mut app = Self {
            width,
            height,
            tab: ControlTab::Display,
            btn_disp: Button::new(Rect::new(18, 92, 168, 30), "Display & Screen"),
            btn_audio: Button::new(Rect::new(18, 128, 168, 30), "Sound & Audio"),
            btn_runit: Button::new(Rect::new(18, 164, 168, 30), "System Services"),
            btn_xbps: Button::new(Rect::new(18, 200, 168, 30), "Software (XBPS)"),
            drm_info: Vec::new(),
            active_res: String::from("1440x900"),
            res_buttons: [
                Button::new(Rect::new(224, 198, 102, 32), "1024x768"),
                Button::new(Rect::new(334, 198, 102, 32), "1280x800"),
                Button::new(Rect::new(444, 198, 102, 32), "1440x900"),
                Button::new(Rect::new(224, 238, 102, 32), "1600x900"),
                Button::new(Rect::new(334, 238, 108, 32), "1920x1080"),
            ],
            volume: 75,
            muted: false,
            btn_vol_down: Button::new(Rect::new(224, 174, 84, 32), "- 5%"),
            btn_vol_up: Button::new(Rect::new(316, 174, 84, 32), "+ 5%"),
            btn_mute: Button::new(Rect::new(408, 174, 104, 32), "Mute"),
            services: Vec::new(),
            sel_svc: None,
            btn_svc_up: Button::new(Rect::new(224, 118, 88, 28), "Start"),
            btn_svc_down: Button::new(Rect::new(320, 118, 88, 28), "Stop"),
            btn_svc_restart: Button::new(Rect::new(416, 118, 108, 28), "Restart"),
            packages: Vec::new(),
            pkg_scroll: 0,
            status: String::from("Control Panel - PetraOS Luna Edition"),
        };
        app.refresh_drm();
        app.refresh_services();
        app.refresh_packages();
        app
    }

    fn refresh_drm(&mut self) {
        if let Ok(mode) = fs::read_to_string("/run/space-active-mode") {
            let trimmed = mode.trim();
            if !trimmed.is_empty() {
                self.active_res = trimmed.to_string();
            }
        }
        self.drm_info.clear();
        if let Ok(rd) = fs::read_dir("/sys/class/drm") {
            for ent in rd.flatten() {
                let name = ent.file_name().to_string_lossy().into_owned();
                let status = fs::read_to_string(ent.path().join("status"))
                    .map(|s| s.trim().to_string())
                    .unwrap_or_default();
                let modes = fs::read_to_string(ent.path().join("modes"))
                    .map(|s| s.lines().next().unwrap_or("").to_string())
                    .unwrap_or_default();
                if !status.is_empty() || !modes.is_empty() {
                    self.drm_info.push(format!("{}: {} ({})", name, status, modes));
                }
            }
        }
        if self.drm_info.is_empty() {
            self.drm_info
                .push(format!("card0: connected (active {})", self.active_res));
        }
    }

    fn refresh_services(&mut self) {
        self.services = RunitManager::list_services();
    }

    fn refresh_packages(&mut self) {
        self.packages = XbpsClient::list_installed();
    }

    fn apply_volume(&mut self) {
        let val = if self.muted { 0 } else { self.volume };
        let _ = fs::write("/run/space-audio-vol", format!("{}\n", val));
        let _ = Command::new("amixer")
            .args(["sset", "Master", &format!("{}%", val)])
            .output();
        self.status = if self.muted {
            "Audio muted".to_string()
        } else {
            format!("Master volume: {}%", self.volume)
        };
    }
}

impl Lifecycle for ControlApp {}

impl Paintable for ControlApp {
    fn paint(&self, canvas: &mut dyn Canvas) {
        let t = Theme::XP_LUNA;
        let w = self.width as i32;
        let h = self.height as i32;

        // 1. Classic XP Menu Bar & Address Bar
        let menu_r = Rect::new(0, 0, w, 24);
        canvas.fill_rect(menu_r, t.bg_window);
        font::draw_text(
            canvas,
            10,
            4,
            "File    Edit    View    Favorites    Tools    Help",
            t.text_primary,
            2,
        );
        canvas.fill_rect(Rect::new(0, 24, w, 1), t.bevel_shadow);

        let addr_bar = Rect::new(0, 25, w, 28);
        canvas.fill_rect(addr_bar, t.bg_window);
        font::draw_text(canvas, 10, 31, "Address", t.text_secondary, 2);
        let addr_well = Rect::new(72, 28, w - 82, 22);
        canvas.fill_rect(addr_well, Color::WHITE);
        canvas.draw_rect_outline(addr_well, 1, t.border);
        icons::draw_icon(canvas, addr_well.x + 4, addr_well.y + 3, 16, IconId::ControlPanel);
        font::draw_text(canvas, addr_well.x + 24, addr_well.y + 3, "Control Panel", t.text_primary, 2);
        canvas.fill_rect(Rect::new(0, 53, w, 1), t.bevel_shadow);

        // 2. Left XP Explorer Task Pane (Blue Gradient Sidebar)
        let side_r = Rect::new(8, 60, 188, h - 88);
        canvas.fill_gradient_v(side_r, Color::rgb(0x7B, 0xA2, 0xE7), Color::rgb(0x63, 0x75, 0xD6));
        canvas.draw_rect_outline(side_r, 1, Color::rgb(0x4E, 0x64, 0xC4));

        // Task Box Header ("Control Panel")
        let card_hdr = Rect::new(side_r.x + 6, side_r.y + 6, side_r.w - 12, 22);
        canvas.fill_gradient_v(card_hdr, Color::WHITE, Color::rgb(0xD6, 0xE3, 0xF7));
        font::draw_text_bold(canvas, card_hdr.x + 8, card_hdr.y + 3, "Control Panel", Color::rgb(0x21, 0x5D, 0xC6), 2);

        let card_body = Rect::new(side_r.x + 6, card_hdr.bottom(), side_r.w - 12, 152);
        canvas.fill_rect(card_body, Color::rgb(0xD6, 0xDF, 0xF7));
        canvas.draw_rect_outline(card_body, 1, Color::WHITE);

        let mut b_disp = self.btn_disp.clone();
        b_disp.active = self.tab == ControlTab::Display;
        let mut b_aud = self.btn_audio.clone();
        b_aud.active = self.tab == ControlTab::Audio;
        let mut b_run = self.btn_runit.clone();
        b_run.active = self.tab == ControlTab::Runit;
        let mut b_xbp = self.btn_xbps.clone();
        b_xbp.active = self.tab == ControlTab::Xbps;

        b_disp.paint(canvas);
        b_aud.paint(canvas);
        b_run.paint(canvas);
        b_xbp.paint(canvas);

        // Second sidebar card ("System Details")
        let info_hdr = Rect::new(side_r.x + 6, card_body.bottom() + 10, side_r.w - 12, 22);
        canvas.fill_gradient_v(info_hdr, Color::WHITE, Color::rgb(0xD6, 0xE3, 0xF7));
        font::draw_text_bold(canvas, info_hdr.x + 8, info_hdr.y + 3, "System Details", Color::rgb(0x21, 0x5D, 0xC6), 2);
        let info_body = Rect::new(side_r.x + 6, info_hdr.bottom(), side_r.w - 12, (side_r.bottom() - info_hdr.bottom() - 6).max(36));
        canvas.fill_rect(info_body, Color::rgb(0xD6, 0xDF, 0xF7));
        canvas.draw_rect_outline(info_body, 1, Color::WHITE);
        font::draw_text(canvas, info_body.x + 8, info_body.y + 8, "PetraOS x86_64", Color::rgb(0x1E, 0x3E, 0x8C), 2);
        font::draw_text(canvas, info_body.x + 8, info_body.y + 28, "Theme: XP Luna Blue", Color::rgb(0x1E, 0x3E, 0x8C), 1);
        font::draw_text(canvas, info_body.x + 8, info_body.y + 44, &format!("Screen: {}", self.active_res), Color::rgb(0x1E, 0x3E, 0x8C), 1);

        // 3. Right Main XP Category View Area
        let body = Rect::new(204, 60, w - 212, h - 88);
        canvas.fill_rect(body, t.bg_surface);
        draw_panel_3d(canvas, body, true, &t);
        canvas.draw_rect_outline(body, 1, t.border);

        // Top XP Royal Periwinkle Banner inside Main Panel
        let banner = Rect::new(body.x + 2, body.y + 2, body.w - 4, 48);
        canvas.fill_gradient_v(banner, Color::rgb(0x5A, 0x72, 0xD6), Color::rgb(0x6B, 0x82, 0xE0));
        let (title_str, icon_id) = match self.tab {
            ControlTab::Display => ("Pick a Display Resolution & DRM Mode", IconId::MyComputer),
            ControlTab::Audio => ("Adjust Master Audio Volume", IconId::ControlPanel),
            ControlTab::Runit => ("Manage Runit System Services", IconId::Tasks),
            ControlTab::Xbps => ("Installed Void Linux XBPS Packages", IconId::Files),
        };
        icons::draw_icon(canvas, banner.x + 12, banner.y + 8, 32, icon_id);
        font::draw_text_shadow(
            canvas,
            banner.x + 52,
            banner.y + 15,
            title_str,
            Color::WHITE,
            Color::rgb(0x24, 0x34, 0x82),
            2,
            true,
        );

        match self.tab {
            ControlTab::Display => {
                font::draw_text_bold(
                    canvas,
                    224,
                    122,
                    &format!("Active Screen Resolution: {}", self.active_res),
                    t.accent,
                    2,
                );
                for (i, line) in self.drm_info.iter().take(2).enumerate() {
                    font::draw_text(canvas, 224, 146 + i as i32 * 20, line, t.text_secondary, 2);
                }
                font::draw_text_bold(
                    canvas,
                    224,
                    174,
                    "Click a resolution to resize screen immediately:",
                    t.text_primary,
                    2,
                );
                for (i, btn) in self.res_buttons.iter().enumerate() {
                    let mut b = btn.clone();
                    b.active = self.active_res == RES_MODES[i];
                    b.paint(canvas);
                }
            }
            ControlTab::Audio => {
                font::draw_text_bold(canvas, 224, 122, "Master Audio Output", t.accent, 2);
                let ratio = if self.muted { 0.0 } else { self.volume as f32 / 100.0 };
                let lbl = if self.muted {
                    "Muted".to_string()
                } else {
                    format!("Volume: {}%", self.volume)
                };
                draw_progress_bar(canvas, Rect::new(224, 138, 360, 26), ratio, &lbl, t.accent);
                self.btn_vol_down.paint(canvas);
                self.btn_vol_up.paint(canvas);
                self.btn_mute.paint(canvas);
            }
            ControlTab::Runit => {
                self.btn_svc_up.paint(canvas);
                self.btn_svc_down.paint(canvas);
                self.btn_svc_restart.paint(canvas);
                let list_r = Rect::new(218, 154, body.w - 28, body.h - 102);
                for (i, svc) in self.services.iter().take(12).enumerate() {
                    let rr = Rect::new(list_r.x, list_r.y + i as i32 * 24, list_r.w, 22);
                    let selected = self.sel_svc == Some(i);
                    if selected {
                        canvas.fill_rect(rr, t.bg_active);
                    }
                    let line = format!("{:<22} {}", svc.name, svc.status);
                    font::draw_text_mono(
                        canvas,
                        rr.x + 6,
                        rr.y + 3,
                        &line,
                        if selected { Color::WHITE } else { t.text_primary },
                        2,
                    );
                }
            }
            ControlTab::Xbps => {
                font::draw_text_bold(
                    canvas,
                    224,
                    118,
                    &format!("Installed XBPS Packages: {}", self.packages.len()),
                    t.accent,
                    2,
                );
                for (i, pkg) in self.packages.iter().skip(self.pkg_scroll).take(13).enumerate() {
                    let line = format!(
                        "{:<24} {}",
                        pkg.name.chars().take(23).collect::<String>(),
                        pkg.version
                    );
                    font::draw_text_mono(canvas, 224, 146 + i as i32 * 22, &line, t.text_primary, 2);
                }
            }
        }

        // Bottom Status Bar
        font::draw_text(canvas, 10, h - 22, &self.status, t.text_secondary, 2);
    }
}

impl AppLogic for ControlApp {
    fn handle_event(&mut self, event: UiEvent) -> bool {
        let mut dirty = false;
        dirty |= self.btn_disp.on_event(event);
        dirty |= self.btn_audio.on_event(event);
        dirty |= self.btn_runit.on_event(event);
        dirty |= self.btn_xbps.on_event(event);

        if self.btn_disp.take_click() {
            self.tab = ControlTab::Display;
            self.refresh_drm();
            return true;
        }
        if self.btn_audio.take_click() {
            self.tab = ControlTab::Audio;
            return true;
        }
        if self.btn_runit.take_click() {
            self.tab = ControlTab::Runit;
            self.refresh_services();
            return true;
        }
        if self.btn_xbps.take_click() {
            self.tab = ControlTab::Xbps;
            self.refresh_packages();
            return true;
        }

        match self.tab {
            ControlTab::Display => {
                for (i, btn) in self.res_buttons.iter_mut().enumerate() {
                    dirty |= btn.on_event(event);
                    if btn.take_click() {
                        let mode_str = RES_MODES[i];
                        let _ = fs::write("/run/space-display-mode", format!("{}\n", mode_str));
                        self.active_res = mode_str.to_string();
                        self.status = format!("Switched screen resolution to {}", mode_str);
                        return true;
                    }
                }
            }
            ControlTab::Audio => {
                dirty |= self.btn_vol_down.on_event(event);
                dirty |= self.btn_vol_up.on_event(event);
                dirty |= self.btn_mute.on_event(event);
                if self.btn_vol_down.take_click() {
                    self.volume = (self.volume - 5).max(0);
                    self.apply_volume();
                    return true;
                }
                if self.btn_vol_up.take_click() {
                    self.volume = (self.volume + 5).min(100);
                    self.apply_volume();
                    return true;
                }
                if self.btn_mute.take_click() {
                    self.muted = !self.muted;
                    self.apply_volume();
                    return true;
                }
            }
            ControlTab::Runit => {
                dirty |= self.btn_svc_up.on_event(event);
                dirty |= self.btn_svc_down.on_event(event);
                dirty |= self.btn_svc_restart.on_event(event);
                let action = if self.btn_svc_up.take_click() {
                    Some("up")
                } else if self.btn_svc_down.take_click() {
                    Some("down")
                } else if self.btn_svc_restart.take_click() {
                    Some("restart")
                } else {
                    None
                };
                if let Some(act) = action {
                    if let Some(sel) = self.sel_svc {
                        if let Some(svc) = self.services.get(sel) {
                            match RunitManager::control(&svc.name, act) {
                                Ok(msg) => self.status = format!("{}: {}", svc.name, msg),
                                Err(e) => self.status = format!("sv error: {}", e),
                            }
                            self.refresh_services();
                        }
                    }
                    return true;
                }
                if let UiEvent::MouseDown { pos, button: 1 } = event {
                    let list_r = Rect::new(218, 154, self.width as i32 - 240, self.height as i32 - 190);
                    if let Some(hit) = list_item_at(list_r, 24, 0, self.services.len(), pos) {
                        self.sel_svc = Some(hit);
                        return true;
                    }
                }
            }
            ControlTab::Xbps => {
                if let UiEvent::Wheel { delta, .. } = event {
                    if delta > 0 {
                        self.pkg_scroll = self.pkg_scroll.saturating_sub(3);
                    } else if delta < 0 && self.pkg_scroll + 1 < self.packages.len() {
                        self.pkg_scroll += 3;
                    }
                    return true;
                }
            }
        }

        dirty
    }
}

fn main() {
    let mut client = SpaceClient::connect("Control Panel", 720, 480).expect("connect failed");
    let mut app = ControlApp::new(720, 480);
    client.run(&mut app);
}
