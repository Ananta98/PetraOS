use space_core::font;
use space_core::void_pkg::{PackageInfo, RunitManager, ServiceInfo, XbpsClient};
use space_core::{Canvas, Rect};
use space_ui::{
    draw_progress_bar, list_item_at, AppLogic, Button, EventListener, Lifecycle, Paintable,
    SpaceClient, Theme, UiEvent,
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
            btn_disp: Button::new(Rect::new(8, 8, 120, 30), "Display"),
            btn_audio: Button::new(Rect::new(136, 8, 120, 30), "Audio"),
            btn_runit: Button::new(Rect::new(264, 8, 140, 30), "Services"),
            btn_xbps: Button::new(Rect::new(412, 8, 140, 30), "XBPS Pkgs"),
            drm_info: Vec::new(),
            volume: 75,
            muted: false,
            btn_vol_down: Button::new(Rect::new(24, 140, 80, 32), "- 5%"),
            btn_vol_up: Button::new(Rect::new(112, 140, 80, 32), "+ 5%"),
            btn_mute: Button::new(Rect::new(200, 140, 100, 32), "Mute"),
            services: Vec::new(),
            sel_svc: None,
            btn_svc_up: Button::new(Rect::new(24, 56, 88, 28), "Start"),
            btn_svc_down: Button::new(Rect::new(120, 56, 88, 28), "Stop"),
            btn_svc_restart: Button::new(Rect::new(216, 56, 108, 28), "Restart"),
            packages: Vec::new(),
            pkg_scroll: 0,
            status: String::from("System Control Center"),
        };
        app.refresh_drm();
        app.refresh_services();
        app.refresh_packages();
        app
    }

    fn refresh_drm(&mut self) {
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
            self.drm_info.push("DRM/KMS: /dev/dri/card0 active".to_string());
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
        let t = Theme::BREEZE_DARK;
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

        let body = Rect::new(8, 48, self.width as i32 - 16, self.height as i32 - 80);
        canvas.fill_rounded_rect(body, 6, t.bg_surface);
        canvas.draw_rect_outline(body, 1, t.border);

        match self.tab {
            ControlTab::Display => {
                font::draw_text(canvas, 24, 64, "DRM/KMS Display Outputs", t.accent, 2);
                for (i, line) in self.drm_info.iter().enumerate() {
                    font::draw_text(canvas, 24, 98 + i as i32 * 26, line, t.text_primary, 2);
                }
            }
            ControlTab::Audio => {
                font::draw_text(canvas, 24, 64, "Master Audio Output", t.accent, 2);
                let ratio = if self.muted { 0.0 } else { self.volume as f32 / 100.0 };
                let lbl = if self.muted {
                    "Muted".to_string()
                } else {
                    format!("Volume: {}%", self.volume)
                };
                draw_progress_bar(canvas, Rect::new(24, 98, 360, 28), ratio, &lbl, t.accent);
                self.btn_vol_down.paint(canvas);
                self.btn_vol_up.paint(canvas);
                self.btn_mute.paint(canvas);
            }
            ControlTab::Runit => {
                self.btn_svc_up.paint(canvas);
                self.btn_svc_down.paint(canvas);
                self.btn_svc_restart.paint(canvas);
                let list_r = Rect::new(16, 94, body.w - 16, body.h - 54);
                for (i, svc) in self.services.iter().take(14).enumerate() {
                    let rr = Rect::new(list_r.x, list_r.y + i as i32 * 24, list_r.w, 22);
                    if self.sel_svc == Some(i) {
                        canvas.fill_rounded_rect(rr, 4, t.accent_soft);
                    }
                    let line = format!("{:<22} {}", svc.name, svc.status);
                    font::draw_text(canvas, rr.x + 6, rr.y + 3, &line, t.text_primary, 2);
                }
            }
            ControlTab::Xbps => {
                font::draw_text(
                    canvas,
                    24,
                    60,
                    &format!("Installed XBPS Packages: {}", self.packages.len()),
                    t.accent,
                    2,
                );
                for (i, pkg) in self.packages.iter().skip(self.pkg_scroll).take(15).enumerate() {
                    let line = format!("{:<24} {}", pkg.name.chars().take(23).collect::<String>(), pkg.version);
                    font::draw_text(canvas, 24, 92 + i as i32 * 22, &line, t.text_primary, 2);
                }
            }
        }

        font::draw_text(canvas, 10, self.height as i32 - 22, &self.status, t.text_secondary, 2);
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
                    let list_r = Rect::new(16, 94, self.width as i32 - 32, self.height as i32 - 134);
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
            ControlTab::Display => {}
        }

        dirty
    }
}

fn main() {
    let mut client = SpaceClient::connect("Space Control", 680, 460).expect("connect failed");
    let mut app = ControlApp::new(680, 460);
    client.run(&mut app);
}
