//! PetraOS Control Panel — ReactOS-inspired System & Display Properties + System Applets.

use space_core::font;
use space_core::icons::{self, IconId};
use space_core::void_pkg::{RunitManager, ServiceInfo};
use space_core::{Canvas, Color, Rect};
use space_ui::{
    draw_progress_bar, list_item_at, AppLogic, Button, EventListener, Layoutable, Lifecycle, Menu,
    MenuBar, MenuItem, Paintable, SpaceClient, StatusBar, StylePainter, TabWidget, TextInput,
    Theme, ToolBar, ToolButton, UiEvent,
};
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, PartialEq, Eq)]
enum ControlSection {
    Home,
    Display,
    System,
    Audio,
    Network,
    DateTime,
    Services,
    Desktop,
}

impl ControlSection {
    const NAV: [(Self, &'static str, &'static str); 8] = [
        (Self::Home, "All Settings", "control-panel"),
        (Self::Display, "Display", "display"),
        (Self::System, "System", "my-computer"),
        (Self::Audio, "Sound & Audio", "audio"),
        (Self::Network, "Network", "network"),
        (Self::DateTime, "Date & Time", "clock"),
        (Self::Services, "Services", "tasks"),
        (Self::Desktop, "Desktop", "petra-logo"),
    ];
}

const DEFAULT_MODES: [&str; 6] = [
    "800x600",
    "1024x768",
    "1280x800",
    "1440x900",
    "1600x900",
    "1920x1080",
];

const WALLPAPERS: [(&str, &str); 5] = [
    ("Petra Slate", "petra-slate"),
    ("Deep Cobalt", "deep-cobalt"),
    ("Midnight Basalt", "midnight-basalt"),
    ("Classic Teal", "classic-teal"),
    ("Solid Charcoal", "solid-charcoal"),
];

struct ControlApp {
    width: u32,
    height: u32,
    section: ControlSection,
    menubar: MenuBar,
    toolbar: ToolBar,
    statusbar: StatusBar,
    nav_buttons: Vec<(Button, ControlSection)>,
    home_cards: Vec<(ControlSection, &'static str, &'static str, &'static str)>,

    // Display
    disp_tabs: TabWidget,
    drm_modes: Vec<String>,
    drm_info: Vec<String>,
    active_res: String,
    pending_res: String,
    res_buttons: Vec<(Button, String)>,
    btn_apply_res: Button,
    appearance_dark: bool,
    btn_theme_light: Button,
    btn_theme_dark: Button,
    active_wallpaper: String,
    wp_buttons: Vec<(Button, &'static str)>,

    // System
    sys_tabs: TabWidget,
    os_name: String,
    kernel_ver: String,
    cpu_model: String,
    mem_total: String,
    mem_avail: String,
    uptime_str: String,
    hostname_input: TextInput,
    btn_save_host: Button,
    hw_items: Vec<String>,
    btn_open_apps: Button,

    // Audio
    volume: i32,
    muted: bool,
    btn_vol_m10: Button,
    btn_vol_m5: Button,
    btn_vol_p5: Button,
    btn_vol_p10: Button,
    btn_mute: Button,
    preset_vol_btns: Vec<(Button, i32)>,
    audio_cards: Vec<String>,

    // Network
    net_ifaces: Vec<String>,
    dns_input: TextInput,
    btn_save_dns: Button,
    btn_net_test: Button,
    net_test_result: String,

    // Date & Time
    btn_refresh_time: Button,

    // Services
    services: Vec<ServiceInfo>,
    sel_svc: Option<usize>,
    svc_scroll: usize,
    btn_svc_up: Button,
    btn_svc_down: Button,
    btn_svc_restart: Button,
    btn_svc_enable: Button,
    btn_svc_disable: Button,

    // Desktop
    btn_layout_float: Button,
    btn_layout_tile: Button,
    btn_launch_term: Button,
    btn_launch_files: Button,
    btn_launch_tasks: Button,
}

impl ControlApp {
    fn new(width: u32, height: u32) -> Self {
        let menubar = MenuBar::new(
            Rect::new(0, 0, 0, 0),
            vec![
                Menu::new(
                    "File",
                    vec![
                        MenuItem::action("refresh", "Refresh All", "F5"),
                        MenuItem::separator(),
                        MenuItem::action("exit", "Exit", "Alt+F4"),
                    ],
                ),
                Menu::new(
                    "Settings",
                    vec![
                        MenuItem::action("sec_display", "Display Settings", ""),
                        MenuItem::action("sec_system", "System Properties", ""),
                        MenuItem::action("sec_audio", "Sound & Audio", ""),
                        MenuItem::action("sec_network", "Network & DNS", ""),
                        MenuItem::action("sec_datetime", "Date & Time", ""),
                        MenuItem::action("sec_services", "System Services", ""),
                        MenuItem::action("sec_desktop", "Desktop & Layout", ""),
                    ],
                ),
                Menu::new(
                    "Tools",
                    vec![
                        MenuItem::action("open_apps", "Applications Manager", ""),
                        MenuItem::action("open_tasks", "Task Manager", ""),
                        MenuItem::action("open_term", "Terminal", ""),
                    ],
                ),
            ],
        );

        let toolbar = ToolBar::new(
            Rect::new(0, 0, 0, 0),
            vec![
                ToolButton::new("sec_home", "Home").with_icon("home"),
                ToolButton::new("sec_display", "Display").with_icon("display"),
                ToolButton::new("sec_system", "System").with_icon("my-computer"),
                ToolButton::new("sec_audio", "Audio").with_icon("audio"),
                ToolButton::new("sec_network", "Network").with_icon("network"),
                ToolButton::new("sec_services", "Services").with_icon("services"),
                ToolButton::new("refresh", "Refresh").with_icon("refresh"),
            ],
        );

        let nav_buttons = ControlSection::NAV
            .iter()
            .map(|&(sec, lbl, ic)| (Button::new(Rect::new(0, 0, 0, 0), lbl).with_icon(ic), sec))
            .collect();

        let home_cards = vec![
            (ControlSection::Display, "Display", "Resolution, Appearance & Wallpaper", "display"),
            (ControlSection::System, "System", "OS Info, Hostname & Hardware", "my-computer"),
            (ControlSection::Audio, "Sound & Audio", "Master Volume & Mixer", "audio"),
            (ControlSection::Network, "Network", "Interfaces, IP & DNS Settings", "network"),
            (ControlSection::DateTime, "Date & Time", "System Clock & Uptime", "clock"),
            (ControlSection::Services, "Services", "Manage Runit Daemons", "tasks"),
            (ControlSection::Desktop, "Desktop", "Window Layout & Shortcuts", "petra-logo"),
        ];

        let mut app = Self {
            width,
            height,
            section: ControlSection::Home,
            menubar,
            toolbar,
            statusbar: StatusBar::new(Rect::new(0, 0, 0, 0)),
            nav_buttons,
            home_cards,

            disp_tabs: TabWidget::new(
                Rect::new(0, 0, 0, 0),
                vec!["Settings".into(), "Appearance".into(), "Background".into()],
            ),
            drm_modes: Vec::new(),
            drm_info: Vec::new(),
            active_res: String::from("1440x900"),
            pending_res: String::from("1440x900"),
            res_buttons: Vec::new(),
            btn_apply_res: Button::new(Rect::new(0, 0, 120, 28), "Apply").with_icon("apply"),
            appearance_dark: false,
            btn_theme_light: Button::new(Rect::new(0, 0, 160, 28), "Petra Slate (Light)").with_icon("theme"),
            btn_theme_dark: Button::new(Rect::new(0, 0, 160, 28), "Petra Basalt (Dark)").with_icon("theme"),
            active_wallpaper: String::from("petra-slate"),
            wp_buttons: Vec::new(),

            sys_tabs: TabWidget::new(
                Rect::new(0, 0, 0, 0),
                vec!["General".into(), "Computer Name".into(), "Hardware".into()],
            ),
            os_name: String::from("PetraOS"),
            kernel_ver: String::from("PetraOS x86_64"),
            cpu_model: String::from("x86_64 CPU"),
            mem_total: String::new(),
            mem_avail: String::new(),
            uptime_str: String::new(),
            hostname_input: TextInput::new(Rect::new(0, 0, 240, 26), "petraos"),
            btn_save_host: Button::new(Rect::new(0, 0, 130, 26), "Save Hostname").with_icon("save"),
            hw_items: Vec::new(),
            btn_open_apps: Button::new(Rect::new(0, 0, 210, 28), "Applications Manager").with_icon("app-manager"),

            volume: 75,
            muted: false,
            btn_vol_m10: Button::new(Rect::new(0, 0, 68, 28), "-10%"),
            btn_vol_m5: Button::new(Rect::new(0, 0, 62, 28), "-5%"),
            btn_vol_p5: Button::new(Rect::new(0, 0, 62, 28), "+5%"),
            btn_vol_p10: Button::new(Rect::new(0, 0, 68, 28), "+10%"),
            btn_mute: Button::new(Rect::new(0, 0, 94, 28), "Mute").with_icon("audio"),
            preset_vol_btns: [25, 50, 75, 100]
                .iter()
                .map(|&v| (Button::new(Rect::new(0, 0, 68, 26), format!("{}%", v)), v))
                .collect(),
            audio_cards: Vec::new(),

            net_ifaces: Vec::new(),
            dns_input: TextInput::new(Rect::new(0, 0, 240, 26), "1.1.1.1"),
            btn_save_dns: Button::new(Rect::new(0, 0, 114, 26), "Save DNS").with_icon("save"),
            btn_net_test: Button::new(Rect::new(0, 0, 146, 28), "Check Network").with_icon("network"),
            net_test_result: String::from("Ready"),

            btn_refresh_time: Button::new(Rect::new(0, 0, 136, 28), "Refresh Clock").with_icon("clock"),

            services: Vec::new(),
            sel_svc: None,
            svc_scroll: 0,
            btn_svc_up: Button::new(Rect::new(0, 0, 82, 26), "Start").with_icon("play"),
            btn_svc_down: Button::new(Rect::new(0, 0, 82, 26), "Stop").with_icon("stop"),
            btn_svc_restart: Button::new(Rect::new(0, 0, 94, 26), "Restart").with_icon("refresh"),
            btn_svc_enable: Button::new(Rect::new(0, 0, 88, 26), "Enable").with_icon("apply"),
            btn_svc_disable: Button::new(Rect::new(0, 0, 90, 26), "Disable").with_icon("close"),

            btn_layout_float: Button::new(Rect::new(0, 0, 168, 30), "Floating Windows").with_icon("show-desktop"),
            btn_layout_tile: Button::new(Rect::new(0, 0, 178, 30), "Tiled Master-Stack").with_icon("view-list"),
            btn_launch_term: Button::new(Rect::new(0, 0, 168, 28), "Open Terminal").with_icon("terminal"),
            btn_launch_files: Button::new(Rect::new(0, 0, 178, 28), "Open File Explorer").with_icon("files"),
            btn_launch_tasks: Button::new(Rect::new(0, 0, 178, 28), "Open Task Manager").with_icon("tasks"),
        };
        app.btn_apply_res.accent = true;
        app.btn_save_host.accent = true;
        app.btn_save_dns.accent = true;
        app.refresh_all();
        app.relayout();
        app
    }

    fn content_rect(&self) -> Rect {
        let w = self.width as i32;
        let h = self.height as i32;
        Rect::new(192, 64, (w - 200).max(320), (h - 94).max(240))
    }

    fn relayout(&mut self) {
        let w = self.width as i32;
        let h = self.height as i32;
        self.menubar.set_bounds(Rect::new(0, 0, w, 24));
        self.toolbar.set_bounds(Rect::new(0, 24, w, 32));
        self.statusbar.set_bounds(Rect::new(0, h - 24, w, 24));

        for (i, (btn, _)) in self.nav_buttons.iter_mut().enumerate() {
            btn.set_bounds(Rect::new(14, 72 + i as i32 * 34, 164, 28));
        }

        let body = self.content_rect();
        let px = body.x + 16;
        let py = body.y + 82;

        // Display
        self.disp_tabs.set_bounds(Rect::new(body.x + 8, body.y + 54, body.w - 16, 26));
        for (i, (btn, _)) in self.res_buttons.iter_mut().enumerate() {
            let col = (i % 3) as i32;
            let row = (i / 3) as i32;
            btn.set_bounds(Rect::new(px + col * 116, py + 148 + row * 34, 108, 28));
        }
        self.btn_apply_res.set_bounds(Rect::new(body.right() - 136, body.bottom() - 40, 120, 28));
        self.btn_theme_light.set_bounds(Rect::new(px, py + 150, 172, 28));
        self.btn_theme_dark.set_bounds(Rect::new(px + 182, py + 150, 172, 28));
        for (i, (btn, _)) in self.wp_buttons.iter_mut().enumerate() {
            let col = (i % 2) as i32;
            let row = (i / 2) as i32;
            btn.set_bounds(Rect::new(px + col * 176, py + 44 + row * 38, 164, 30));
        }

        // System
        self.sys_tabs.set_bounds(Rect::new(body.x + 8, body.y + 54, body.w - 16, 26));
        self.hostname_input.set_bounds(Rect::new(px, py + 46, 240, 26));
        self.btn_save_host.set_bounds(Rect::new(px + 250, py + 46, 130, 26));
        self.btn_open_apps.set_bounds(Rect::new(px, body.bottom() - 42, 210, 28));

        // Audio
        self.btn_vol_m10.set_bounds(Rect::new(px, py + 76, 68, 28));
        self.btn_vol_m5.set_bounds(Rect::new(px + 74, py + 76, 62, 28));
        self.btn_vol_p5.set_bounds(Rect::new(px + 142, py + 76, 62, 28));
        self.btn_vol_p10.set_bounds(Rect::new(px + 210, py + 76, 68, 28));
        self.btn_mute.set_bounds(Rect::new(px + 286, py + 76, 94, 28));
        for (i, (btn, _)) in self.preset_vol_btns.iter_mut().enumerate() {
            btn.set_bounds(Rect::new(px + i as i32 * 74, py + 120, 66, 26));
        }

        // Network
        self.dns_input.set_bounds(Rect::new(px, body.bottom() - 88, 240, 26));
        self.btn_save_dns.set_bounds(Rect::new(px + 250, body.bottom() - 88, 114, 26));
        self.btn_net_test.set_bounds(Rect::new(px, body.bottom() - 48, 146, 28));

        // Date & Time
        self.btn_refresh_time.set_bounds(Rect::new(px, py + 124, 136, 28));

        // Services
        self.btn_svc_up.set_bounds(Rect::new(px, body.y + 62, 82, 26));
        self.btn_svc_down.set_bounds(Rect::new(px + 88, body.y + 62, 82, 26));
        self.btn_svc_restart.set_bounds(Rect::new(px + 176, body.y + 62, 94, 26));
        self.btn_svc_enable.set_bounds(Rect::new(px + 276, body.y + 62, 88, 26));
        self.btn_svc_disable.set_bounds(Rect::new(px + 370, body.y + 62, 90, 26));

        // Desktop
        self.btn_layout_float.set_bounds(Rect::new(px, py + 32, 168, 30));
        self.btn_layout_tile.set_bounds(Rect::new(px + 178, py + 32, 178, 30));
        self.btn_launch_term.set_bounds(Rect::new(px, py + 108, 168, 28));
        self.btn_launch_files.set_bounds(Rect::new(px + 178, py + 108, 178, 28));
        self.btn_launch_tasks.set_bounds(Rect::new(px, py + 144, 168, 28));
    }

    fn refresh_all(&mut self) {
        self.refresh_drm();
        self.refresh_system();
        self.refresh_audio();
        self.refresh_network();
        self.services = RunitManager::list_services();
        self.statusbar.set_text(0, "PetraOS Control Panel");
        self.statusbar.set_text(1, format!("Screen: {}", self.active_res));
    }

    fn refresh_drm(&mut self) {
        if let Ok(mode) = fs::read_to_string("/run/space-active-mode") {
            let trimmed = mode.trim();
            if !trimmed.is_empty() {
                self.active_res = trimmed.to_string();
                self.pending_res = trimmed.to_string();
            }
        }
        if let Ok(wp) = fs::read_to_string("/run/space-wallpaper") {
            let trimmed = wp.trim();
            if !trimmed.is_empty() {
                self.active_wallpaper = trimmed.to_string();
            }
        }
        self.drm_info.clear();
        self.drm_modes.clear();
        if let Ok(rd) = fs::read_dir("/sys/class/drm") {
            for ent in rd.flatten() {
                let name = ent.file_name().to_string_lossy().into_owned();
                let status = fs::read_to_string(ent.path().join("status"))
                    .map(|s| s.trim().to_string())
                    .unwrap_or_default();
                let modes_txt = fs::read_to_string(ent.path().join("modes")).unwrap_or_default();
                for line in modes_txt.lines() {
                    let m = line.trim();
                    if m.contains('x') && !self.drm_modes.contains(&m.to_string()) && self.drm_modes.len() < 9 {
                        self.drm_modes.push(m.to_string());
                    }
                }
                if !status.is_empty() || !modes_txt.is_empty() {
                    let first = modes_txt.lines().next().unwrap_or("");
                    self.drm_info.push(format!("{}: {} ({})", name, status, first));
                }
            }
        }
        for &m in &DEFAULT_MODES {
            if !self.drm_modes.contains(&m.to_string()) && self.drm_modes.len() < 9 {
                self.drm_modes.push(m.to_string());
            }
        }
        if self.drm_info.is_empty() {
            self.drm_info.push(format!("VirtIO/DRM Display: connected ({})", self.active_res));
        }

        self.res_buttons = self
            .drm_modes
            .iter()
            .take(6)
            .map(|m| (Button::new(Rect::new(0, 0, 108, 28), m.clone()).with_icon("display"), m.clone()))
            .collect();

        self.wp_buttons = WALLPAPERS
            .iter()
            .map(|&(lbl, id)| (Button::new(Rect::new(0, 0, 164, 30), lbl).with_icon("theme"), id))
            .collect();
        self.relayout();
    }

    fn refresh_system(&mut self) {
        if let Ok(rel) = fs::read_to_string("/usr/lib/os-release").or_else(|_| fs::read_to_string("/etc/os-release")) {
            for line in rel.lines() {
                if let Some(val) = line.strip_prefix("PRETTY_NAME=") {
                    self.os_name = val.trim_matches('"').to_string();
                }
            }
        }
        if let Ok(ver) = fs::read_to_string("/proc/version") {
            self.kernel_ver = ver.split_whitespace().take(3).collect::<Vec<_>>().join(" ");
        }
        if let Ok(cpu) = fs::read_to_string("/proc/cpuinfo") {
            for line in cpu.lines() {
                if let Some(val) = line.split_once(':').filter(|(k, _)| k.trim() == "model name").map(|(_, v)| v.trim()) {
                    self.cpu_model = val.to_string();
                    break;
                }
            }
        }
        if let Ok(mem) = fs::read_to_string("/proc/meminfo") {
            for line in mem.lines() {
                if let Some(rest) = line.strip_prefix("MemTotal:") {
                    if let Some(kb) = rest.split_whitespace().next().and_then(|s| s.parse::<u64>().ok()) {
                        self.mem_total = format!("{} MB", kb / 1024);
                    }
                } else if let Some(rest) = line.strip_prefix("MemAvailable:") {
                    if let Some(kb) = rest.split_whitespace().next().and_then(|s| s.parse::<u64>().ok()) {
                        self.mem_avail = format!("{} MB available", kb / 1024);
                    }
                }
            }
        }
        if let Ok(up) = fs::read_to_string("/proc/uptime") {
            if let Some(secs_f) = up.split_whitespace().next().and_then(|s| s.parse::<f64>().ok()) {
                let s = secs_f as u64;
                let h = s / 3600;
                let m = (s % 3600) / 60;
                let sec = s % 60;
                self.uptime_str = format!("{}h {:02}m {:02}s", h, m, sec);
            }
        }
        if let Ok(hn) = fs::read_to_string("/etc/hostname") {
            self.hostname_input.text = hn.trim().to_string();
        }
        self.hw_items.clear();
        for (cls, label) in [
            ("/sys/class/drm", "Display"),
            ("/sys/class/net", "Network"),
            ("/sys/class/block", "Block"),
            ("/sys/class/input", "Input"),
        ] {
            if let Ok(rd) = fs::read_dir(cls) {
                for ent in rd.flatten().take(4) {
                    self.hw_items.push(format!("[{}] {}", label, ent.file_name().to_string_lossy()));
                }
            }
        }
        if self.hw_items.is_empty() {
            self.hw_items.push("VirtIO GPU, AHCI Disk, e1000/VirtIO Net, Evdev Input".to_string());
        }
    }

    fn refresh_audio(&mut self) {
        if let Ok(v) = fs::read_to_string("/run/space-audio-vol") {
            if let Ok(val) = v.trim().parse::<i32>() {
                self.muted = val == 0;
                if val > 0 {
                    self.volume = val.clamp(0, 100);
                }
            }
        }
        self.audio_cards.clear();
        if let Ok(cards) = fs::read_to_string("/proc/asound/cards") {
            for l in cards.lines().take(4) {
                self.audio_cards.push(l.trim().to_string());
            }
        }
        if self.audio_cards.is_empty() {
            self.audio_cards.push("VirtIO-Sound / PCM Master Output".to_string());
        }
    }

    fn refresh_network(&mut self) {
        self.net_ifaces.clear();
        if let Ok(rd) = fs::read_dir("/sys/class/net") {
            for ent in rd.flatten() {
                let name = ent.file_name().to_string_lossy().into_owned();
                let addr = fs::read_to_string(ent.path().join("address"))
                    .map(|s| s.trim().to_string())
                    .unwrap_or_else(|_| "00:00:00:00:00:00".into());
                let state = fs::read_to_string(ent.path().join("operstate"))
                    .map(|s| s.trim().to_string())
                    .unwrap_or_else(|_| "up".into());
                self.net_ifaces.push(format!("{}  MAC: {}  State: {}", name, addr, state));
            }
        }
        if self.net_ifaces.is_empty() {
            self.net_ifaces.push("eth0  IP: 10.0.2.15/24  GW: 10.0.2.2".to_string());
        }
        if let Ok(resolv) = fs::read_to_string("/etc/resolv.conf") {
            let servers: Vec<&str> = resolv
                .lines()
                .filter_map(|l| l.trim().strip_prefix("nameserver "))
                .map(|s| s.trim())
                .collect();
            if !servers.is_empty() {
                self.dns_input.text = servers.join(" ");
            }
        }
    }

    fn apply_volume(&mut self) {
        let val = if self.muted { 0 } else { self.volume };
        let _ = fs::write("/run/space-audio-vol", format!("{}\n", val));
        let _ = Command::new("amixer")
            .args(["sset", "Master", &format!("{}%", val)])
            .output();
        let msg = if self.muted {
            "Audio muted".to_string()
        } else {
            format!("Master volume set to {}%", self.volume)
        };
        self.statusbar.set_text(1, msg);
    }

    fn utc_now_string() -> String {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let days = secs / 86_400;
        let rem = secs % 86_400;
        let h = rem / 3600;
        let m = (rem % 3600) / 60;
        let s = rem % 60;
        // Civil-from-days (Howard Hinnant algorithm)
        let z = days as i64 + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let mo = mp + if mp < 10 { 3 } else { -9 };
        let year = y + if mo <= 2 { 1 } else { 0 };
        format!("{:04}-{:02}-{:02}  {:02}:{:02}:{:02} UTC", year, mo, d, h, m, s)
    }

    fn paint_banner(&self, canvas: &mut dyn Canvas, body: Rect, title: &str, subtitle: &str, icon: &'static str, t: &Theme) {
        let banner = Rect::new(body.x + 1, body.y + 1, body.w - 2, 48);
        canvas.fill_gradient_v(banner, Theme::TITLE_ACTIVE_TOP, Theme::TITLE_ACTIVE_BOT);
        canvas.fill_rect(Rect::new(banner.x, banner.bottom() - 1, banner.w, 1), Theme::TITLE_ACTIVE_BORDER);
        icons::draw_icon(canvas, banner.x + 12, banner.y + 8, 32, &IconId::new(icon));
        font::draw_text_shadow(
            canvas,
            banner.x + 52,
            banner.y + 7,
            title,
            Color::WHITE,
            Color::rgb(0x10, 0x26, 0x4C),
            2,
            true,
        );
        font::draw_text(
            canvas,
            banner.x + 52,
            banner.y + 27,
            subtitle,
            Color::rgb(0xC8, 0xDF, 0xFA),
            1,
        );
        let _ = t;
    }

    fn paint_display_section(&self, canvas: &mut dyn Canvas, body: Rect, t: &Theme) {
        self.paint_banner(
            canvas,
            body,
            "Properties for Display",
            "Resolution, Visual Appearance & Desktop Background",
            "display",
            t,
        );
        self.disp_tabs.paint_with(canvas, t);
        let px = body.x + 16;
        let py = body.y + 88;

        match self.disp_tabs.active {
            0 => {
                // Monitor aspect preview box (ReactOS Display Settings style)
                let mon_outer = Rect::new(px + 90, py + 4, 152, 98);
                canvas.fill_rounded_rect(mon_outer, 4, Color::rgb(0x2A, 0x34, 0x42));
                canvas.draw_rect_outline(mon_outer, 1, Color::rgb(0x16, 0x1E, 0x28));
                let scr = Rect::new(mon_outer.x + 8, mon_outer.y + 8, mon_outer.w - 16, mon_outer.h - 20);
                canvas.fill_gradient_v(scr, Theme::TITLE_ACTIVE_TOP, Theme::TITLE_ACTIVE_BOT);
                let rw = font::measure_text_width(&self.pending_res, 2);
                font::draw_text_bold(
                    canvas,
                    scr.x + (scr.w - rw) / 2,
                    scr.y + 26,
                    &self.pending_res,
                    Color::WHITE,
                    2,
                );
                canvas.fill_rect(Rect::new(mon_outer.x + 62, mon_outer.bottom(), 28, 10), Color::rgb(0x66, 0x72, 0x82));
                canvas.fill_rect(Rect::new(mon_outer.x + 42, mon_outer.bottom() + 10, 68, 6), Color::rgb(0x4E, 0x58, 0x66));

                font::draw_text_bold(
                    canvas,
                    px,
                    py + 122,
                    &format!("Active: {}   |   Selected: {}", self.active_res, self.pending_res),
                    t.accent,
                    2,
                );
                for (btn, mode) in &self.res_buttons {
                    let mut b = btn.clone();
                    b.active = mode == &self.pending_res;
                    b.paint_with(canvas, t);
                }
                if let Some(info) = self.drm_info.first() {
                    font::draw_text(canvas, px, body.bottom() - 36, info, t.text_secondary, 1);
                }
                self.btn_apply_res.paint_with(canvas, t);
            }
            1 => {
                // Appearance mini-windows preview (matching Image 5!)
                let prev = Rect::new(px, py + 4, body.w - 32, 128);
                canvas.fill_rect(prev, Color::rgb(0x3D, 0x52, 0x6B));
                canvas.draw_rect_outline(prev, 1, t.border);

                let inact = Rect::new(prev.x + 14, prev.y + 10, 210, 74);
                canvas.fill_rect(inact, t.bg_window);
                canvas.fill_gradient_v(Rect::new(inact.x, inact.y, inact.w, 20), Theme::TITLE_INACTIVE_TOP, Theme::TITLE_INACTIVE_BOT);
                canvas.draw_rect_outline(inact, 1, Theme::TITLE_INACTIVE_BORDER);
                font::draw_text(canvas, inact.x + 8, inact.y + 4, "Inactive Window", Color::rgb(0x3D, 0x47, 0x56), 1);

                let act_t = if self.appearance_dark { Theme::PETRA_BASALT } else { Theme::PETRA_SLATE };
                let act = Rect::new(prev.x + 44, prev.y + 32, 230, 82);
                canvas.fill_rect(act, act_t.bg_window);
                canvas.fill_gradient_v(Rect::new(act.x, act.y, act.w, 20), Theme::TITLE_ACTIVE_TOP, Theme::TITLE_ACTIVE_BOT);
                canvas.draw_rect_outline(act, 1, Theme::TITLE_ACTIVE_BORDER);
                font::draw_text_bold(canvas, act.x + 8, act.y + 4, "Active Window", Color::WHITE, 1);
                font::draw_text(canvas, act.x + 10, act.y + 30, "Window Text", act_t.text_primary, 1);

                let mut b_light = self.btn_theme_light.clone();
                b_light.active = !self.appearance_dark;
                b_light.paint_with(canvas, t);
                let mut b_dark = self.btn_theme_dark.clone();
                b_dark.active = self.appearance_dark;
                b_dark.paint_with(canvas, t);
            }
            _ => {
                font::draw_text_bold(canvas, px, py + 12, "Select Desktop Wallpaper Style:", t.text_primary, 2);
                for (btn, id) in &self.wp_buttons {
                    let mut b = btn.clone();
                    b.active = *id == self.active_wallpaper;
                    b.paint_with(canvas, t);
                }
            }
        }
    }

    fn paint_system_section(&self, canvas: &mut dyn Canvas, body: Rect, t: &Theme) {
        self.paint_banner(
            canvas,
            body,
            "Properties for System",
            "Operating System, Hardware & Computer Identity",
            "my-computer",
            t,
        );
        self.sys_tabs.paint_with(canvas, t);
        let px = body.x + 16;
        let py = body.y + 88;

        match self.sys_tabs.active {
            0 => {
                icons::draw_icon(canvas, px + 8, py + 10, 56, &IconId::new("petra-logo"));
                font::draw_text_bold(canvas, px + 76, py + 14, &self.os_name, t.accent, 3);
                font::draw_text(canvas, px + 76, py + 44, "PetraSpace Desktop Environment", t.text_secondary, 2);

                let rows = [
                    ("Kernel:", self.kernel_ver.as_str()),
                    ("Processor:", self.cpu_model.as_str()),
                    ("Memory:", self.mem_total.as_str()),
                    ("Available:", self.mem_avail.as_str()),
                    ("Uptime:", self.uptime_str.as_str()),
                ];
                for (i, (k, v)) in rows.iter().enumerate() {
                    let ry = py + 82 + i as i32 * 26;
                    font::draw_text_bold(canvas, px + 8, ry, k, t.text_secondary, 2);
                    font::draw_text_clipped(canvas, body, px + 112, ry, v, t.text_primary, 2);
                }
                self.btn_open_apps.paint_with(canvas, t);
            }
            1 => {
                font::draw_text_bold(canvas, px, py + 12, "Computer Hostname (/etc/hostname):", t.text_primary, 2);
                self.hostname_input.paint_with(canvas, t);
                self.btn_save_host.paint_with(canvas, t);
                font::draw_text(
                    canvas,
                    px,
                    py + 92,
                    "Changes take effect immediately in /etc/hostname.",
                    t.text_secondary,
                    2,
                );
            }
            _ => {
                font::draw_text_bold(canvas, px, py + 10, "Detected Hardware & Devices (/sys/class):", t.accent, 2);
                for (i, item) in self.hw_items.iter().take(10).enumerate() {
                    font::draw_text_mono(canvas, px, py + 38 + i as i32 * 22, item, t.text_primary, 2);
                }
            }
        }
    }

    fn paint_services_section(&self, canvas: &mut dyn Canvas, body: Rect, t: &Theme) {
        self.paint_banner(
            canvas,
            body,
            "System Services (Runit)",
            "Start, Stop, Restart, Enable or Disable /etc/sv Daemons",
            "tasks",
            t,
        );
        self.btn_svc_up.paint_with(canvas, t);
        self.btn_svc_down.paint_with(canvas, t);
        self.btn_svc_restart.paint_with(canvas, t);
        self.btn_svc_enable.paint_with(canvas, t);
        self.btn_svc_disable.paint_with(canvas, t);

        let list_r = Rect::new(body.x + 12, body.y + 96, body.w - 24, body.h - 108);
        StylePainter::new(t).paint_sunken_field(canvas, list_r, false);
        let max_rows = ((list_r.h - 6) / 24).max(1) as usize;
        for (vi, svc) in self.services.iter().skip(self.svc_scroll).take(max_rows).enumerate() {
            let real_idx = self.svc_scroll + vi;
            let rr = Rect::new(list_r.x + 2, list_r.y + 2 + vi as i32 * 24, list_r.w - 4, 22);
            let selected = self.sel_svc == Some(real_idx);
            StylePainter::new(t).paint_row_bg(canvas, rr, real_idx, selected);
            let en = if svc.enabled { "enabled" } else { "disabled" };
            let line = format!("{:<20} [{:<8}] {}", svc.name, en, svc.status);
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
}

impl Lifecycle for ControlApp {}

impl Paintable for ControlApp {
    fn paint(&self, canvas: &mut dyn Canvas) {
        let t = Theme::PETRA_SLATE;
        let w = self.width as i32;
        let h = self.height as i32;
        canvas.fill_rect(Rect::new(0, 0, w, h), t.bg_window);

        self.menubar.paint_with(canvas, &t);
        self.toolbar.paint_with(canvas, &t);

        // Left category sidebar
        let side_r = Rect::new(8, 64, 176, (h - 94).max(200));
        StylePainter::new(&t).paint_sunken_field(canvas, side_r, false);
        for (btn, sec) in &self.nav_buttons {
            let mut b = btn.clone();
            b.active = *sec == self.section;
            b.paint_with(canvas, &t);
        }

        let body = self.content_rect();
        canvas.fill_rect(body, t.bg_surface);
        StylePainter::new(&t).paint_panel_3d(canvas, body, true);

        let px = body.x + 16;
        let py = body.y + 82;

        match self.section {
            ControlSection::Home => {
                self.paint_banner(
                    canvas,
                    body,
                    "PetraOS Control Panel",
                    "Configure display, hardware, sound, network, and system services",
                    "control-panel",
                    &t,
                );
                let card_w = ((body.w - 44) / 2).max(160);
                let card_h = 68;
                for (i, &(_, title, sub, icon)) in self.home_cards.iter().enumerate() {
                    let col = (i % 2) as i32;
                    let row = (i / 2) as i32;
                    let cr = Rect::new(body.x + 14 + col * (card_w + 12), body.y + 62 + row * (card_h + 10), card_w, card_h);
                    canvas.fill_rect(cr, t.bg_surface_alt);
                    canvas.draw_rect_outline(cr, 1, t.border);
                    icons::draw_icon(canvas, cr.x + 12, cr.y + 18, 32, &IconId::new(icon));
                    font::draw_text_bold(canvas, cr.x + 54, cr.y + 14, title, t.accent, 2);
                    font::draw_text_clipped(canvas, cr, cr.x + 54, cr.y + 36, sub, t.text_secondary, 1);
                }
            }
            ControlSection::Display => self.paint_display_section(canvas, body, &t),
            ControlSection::System => self.paint_system_section(canvas, body, &t),
            ControlSection::Audio => {
                self.paint_banner(canvas, body, "Sound & Audio", "Master Mixer Volume & Audio Hardware", "audio", &t);
                let ratio = if self.muted { 0.0 } else { self.volume as f32 / 100.0 };
                let lbl = if self.muted { "Muted".into() } else { format!("Master Volume: {}%", self.volume) };
                draw_progress_bar(canvas, Rect::new(px, py + 24, (body.w - 32).min(420), 28), ratio, &lbl, t.accent);
                self.btn_vol_m10.paint_with(canvas, &t);
                self.btn_vol_m5.paint_with(canvas, &t);
                self.btn_vol_p5.paint_with(canvas, &t);
                self.btn_vol_p10.paint_with(canvas, &t);
                self.btn_mute.paint_with(canvas, &t);
                for (btn, _) in &self.preset_vol_btns {
                    btn.paint_with(canvas, &t);
                }
                font::draw_text_bold(canvas, px, py + 170, "Audio Devices:", t.text_primary, 2);
                for (i, c) in self.audio_cards.iter().enumerate() {
                    font::draw_text(canvas, px, py + 194 + i as i32 * 20, c, t.text_secondary, 2);
                }
            }
            ControlSection::Network => {
                self.paint_banner(canvas, body, "Network & Internet", "Interfaces, IPv4 & DNS Configuration", "network", &t);
                font::draw_text_bold(canvas, px, body.y + 62, "Active Interfaces:", t.accent, 2);
                for (i, iface) in self.net_ifaces.iter().take(5).enumerate() {
                    font::draw_text_mono(canvas, px, body.y + 88 + i as i32 * 22, iface, t.text_primary, 2);
                }
                font::draw_text_bold(canvas, px, body.bottom() - 112, "DNS Nameservers (/etc/resolv.conf):", t.text_primary, 2);
                self.dns_input.paint_with(canvas, &t);
                self.btn_save_dns.paint_with(canvas, &t);
                self.btn_net_test.paint_with(canvas, &t);
                font::draw_text(canvas, px + 152, body.bottom() - 42, &self.net_test_result, t.text_secondary, 2);
            }
            ControlSection::DateTime => {
                self.paint_banner(canvas, body, "Date & Time", "System Clock & Boot Uptime", "clock", &t);
                font::draw_text_bold(canvas, px, py + 14, "Current System Time:", t.text_secondary, 2);
                font::draw_text_bold(canvas, px, py + 40, &Self::utc_now_string(), t.accent, 3);
                font::draw_text(canvas, px, py + 84, &format!("System Uptime: {}", self.uptime_str), t.text_primary, 2);
                self.btn_refresh_time.paint_with(canvas, &t);
            }
            ControlSection::Services => self.paint_services_section(canvas, body, &t),
            ControlSection::Desktop => {
                self.paint_banner(canvas, body, "Desktop & Window Layout", "Compositor Layout & Quick Tools", "petra-logo", &t);
                font::draw_text_bold(canvas, px, py + 4, "Window Management Layout:", t.text_primary, 2);
                self.btn_layout_float.paint_with(canvas, &t);
                self.btn_layout_tile.paint_with(canvas, &t);
                font::draw_text_bold(canvas, px, py + 82, "Launch System Utilities:", t.text_primary, 2);
                self.btn_launch_term.paint_with(canvas, &t);
                self.btn_launch_files.paint_with(canvas, &t);
                self.btn_launch_tasks.paint_with(canvas, &t);
            }
        }

        self.statusbar.paint_with(canvas, &t);
        self.menubar.paint_popup(canvas);
    }
}

impl AppLogic for ControlApp {
    fn handle_event(&mut self, event: UiEvent) -> bool {
        if let UiEvent::Resize { width, height } = event {
            self.width = width;
            self.height = height;
            self.relayout();
            return true;
        }

        let menu_was_open = self.menubar.is_open();
        let mut dirty = self.menubar.on_event(event);
        if let Some(act) = self.menubar.take_action() {
            match act {
                "exit" => std::process::exit(0),
                "refresh" => {
                    self.refresh_all();
                    return true;
                }
                "sec_display" => { self.section = ControlSection::Display; return true; }
                "sec_system" => { self.section = ControlSection::System; self.refresh_system(); return true; }
                "sec_audio" => { self.section = ControlSection::Audio; return true; }
                "sec_network" => { self.section = ControlSection::Network; self.refresh_network(); return true; }
                "sec_datetime" => { self.section = ControlSection::DateTime; self.refresh_system(); return true; }
                "sec_services" => { self.section = ControlSection::Services; self.services = RunitManager::list_services(); return true; }
                "sec_desktop" => { self.section = ControlSection::Desktop; return true; }
                "open_apps" => { let _ = Command::new("space-apps").spawn(); return true; }
                "open_tasks" => { let _ = Command::new("space-tasks").spawn(); return true; }
                "open_term" => { let _ = Command::new("space-term").spawn(); return true; }
                _ => {}
            }
        }
        if menu_was_open {
            return dirty;
        }

        dirty |= self.toolbar.on_event(event);
        if let Some(act) = self.toolbar.take_action() {
            match act {
                "sec_home" => self.section = ControlSection::Home,
                "sec_display" => { self.section = ControlSection::Display; self.refresh_drm(); }
                "sec_system" => { self.section = ControlSection::System; self.refresh_system(); }
                "sec_audio" => { self.section = ControlSection::Audio; self.refresh_audio(); }
                "sec_network" => { self.section = ControlSection::Network; self.refresh_network(); }
                "sec_services" => { self.section = ControlSection::Services; self.services = RunitManager::list_services(); }
                "refresh" => self.refresh_all(),
                _ => {}
            }
            return true;
        }

        for (btn, sec) in &mut self.nav_buttons {
            dirty |= btn.on_event(event);
            if btn.take_click() {
                self.section = *sec;
                if *sec == ControlSection::Display {
                    self.refresh_drm();
                } else if *sec == ControlSection::System {
                    self.refresh_system();
                } else if *sec == ControlSection::Services {
                    self.services = RunitManager::list_services();
                } else if *sec == ControlSection::Network {
                    self.refresh_network();
                }
                return true;
            }
        }

        let body = self.content_rect();
        match self.section {
            ControlSection::Home => {
                if let UiEvent::MouseDown { pos, button: 1 } = event {
                    let card_w = ((body.w - 44) / 2).max(160);
                    let card_h = 68;
                    for (i, &(sec, ..)) in self.home_cards.iter().enumerate() {
                        let col = (i % 2) as i32;
                        let row = (i / 2) as i32;
                        let cr = Rect::new(body.x + 14 + col * (card_w + 12), body.y + 62 + row * (card_h + 10), card_w, card_h);
                        if cr.contains(pos) {
                            self.section = sec;
                            self.refresh_all();
                            return true;
                        }
                    }
                }
            }
            ControlSection::Display => {
                dirty |= self.disp_tabs.on_event(event);
                if self.disp_tabs.take_changed() {
                    return true;
                }
                match self.disp_tabs.active {
                    0 => {
                        for (btn, mode) in &mut self.res_buttons {
                            dirty |= btn.on_event(event);
                            if btn.take_click() {
                                self.pending_res = mode.clone();
                                return true;
                            }
                        }
                        dirty |= self.btn_apply_res.on_event(event);
                        if self.btn_apply_res.take_click() {
                            let _ = fs::write("/run/space-display-mode", format!("{}\n", self.pending_res));
                            self.active_res = self.pending_res.clone();
                            self.statusbar.set_text(1, format!("Screen: {}", self.active_res));
                            return true;
                        }
                    }
                    1 => {
                        dirty |= self.btn_theme_light.on_event(event);
                        dirty |= self.btn_theme_dark.on_event(event);
                        if self.btn_theme_light.take_click() {
                            self.appearance_dark = false;
                            let _ = fs::write("/run/space-theme", "petra-slate\n");
                            self.statusbar.set_text(1, "Theme: Petra Slate");
                            return true;
                        }
                        if self.btn_theme_dark.take_click() {
                            self.appearance_dark = true;
                            let _ = fs::write("/run/space-theme", "petra-basalt\n");
                            self.statusbar.set_text(1, "Theme: Petra Basalt");
                            return true;
                        }
                    }
                    _ => {
                        for (btn, id) in &mut self.wp_buttons {
                            dirty |= btn.on_event(event);
                            if btn.take_click() {
                                self.active_wallpaper = (*id).to_string();
                                let _ = fs::write("/run/space-wallpaper", format!("{}\n", id));
                                self.statusbar.set_text(1, format!("Wallpaper: {}", id));
                                return true;
                            }
                        }
                    }
                }
            }
            ControlSection::System => {
                dirty |= self.sys_tabs.on_event(event);
                if self.sys_tabs.take_changed() {
                    return true;
                }
                if self.sys_tabs.active == 0 {
                    dirty |= self.btn_open_apps.on_event(event);
                    if self.btn_open_apps.take_click() {
                        let _ = Command::new("space-apps").spawn();
                        return true;
                    }
                } else if self.sys_tabs.active == 1 {
                    dirty |= self.hostname_input.on_event(event);
                    dirty |= self.btn_save_host.on_event(event);
                    if self.btn_save_host.take_click() || self.hostname_input.take_submit() {
                        let hn = self.hostname_input.text.trim();
                        if !hn.is_empty() && fs::write("/etc/hostname", format!("{}\n", hn)).is_ok() {
                            self.statusbar.set_text(1, format!("Saved hostname: {}", hn));
                        }
                        return true;
                    }
                }
            }
            ControlSection::Audio => {
                dirty |= self.btn_vol_m10.on_event(event);
                dirty |= self.btn_vol_m5.on_event(event);
                dirty |= self.btn_vol_p5.on_event(event);
                dirty |= self.btn_vol_p10.on_event(event);
                dirty |= self.btn_mute.on_event(event);
                if self.btn_vol_m10.take_click() { self.volume = (self.volume - 10).max(0); self.muted = false; self.apply_volume(); return true; }
                if self.btn_vol_m5.take_click() { self.volume = (self.volume - 5).max(0); self.muted = false; self.apply_volume(); return true; }
                if self.btn_vol_p5.take_click() { self.volume = (self.volume + 5).min(100); self.muted = false; self.apply_volume(); return true; }
                if self.btn_vol_p10.take_click() { self.volume = (self.volume + 10).min(100); self.muted = false; self.apply_volume(); return true; }
                if self.btn_mute.take_click() { self.muted = !self.muted; self.apply_volume(); return true; }
                for (btn, v) in &mut self.preset_vol_btns {
                    dirty |= btn.on_event(event);
                    if btn.take_click() {
                        self.volume = *v;
                        self.muted = false;
                        self.apply_volume();
                        return true;
                    }
                }
            }
            ControlSection::Network => {
                dirty |= self.dns_input.on_event(event);
                dirty |= self.btn_save_dns.on_event(event);
                dirty |= self.btn_net_test.on_event(event);
                if self.btn_save_dns.take_click() || self.dns_input.take_submit() {
                    let mut content = String::new();
                    for ns in self.dns_input.text.split_whitespace() {
                        content.push_str(&format!("nameserver {}\n", ns));
                    }
                    if !content.is_empty() && fs::write("/etc/resolv.conf", content).is_ok() {
                        self.statusbar.set_text(1, "Updated /etc/resolv.conf");
                    }
                    return true;
                }
                if self.btn_net_test.take_click() {
                    let ok = std::net::TcpStream::connect_timeout(
                        &std::net::SocketAddr::from(([1, 1, 1, 1], 443)),
                        std::time::Duration::from_secs(2),
                    )
                    .is_ok();
                    self.net_test_result = if ok {
                        "Online (TCP 1.1.1.1:443 reachable)".to_string()
                    } else {
                        "Offline or unreachable".to_string()
                    };
                    return true;
                }
            }
            ControlSection::DateTime => {
                dirty |= self.btn_refresh_time.on_event(event);
                if self.btn_refresh_time.take_click() {
                    self.refresh_system();
                    return true;
                }
            }
            ControlSection::Services => {
                dirty |= self.btn_svc_up.on_event(event);
                dirty |= self.btn_svc_down.on_event(event);
                dirty |= self.btn_svc_restart.on_event(event);
                dirty |= self.btn_svc_enable.on_event(event);
                dirty |= self.btn_svc_disable.on_event(event);

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
                    if let Some(svc) = self.sel_svc.and_then(|i| self.services.get(i)) {
                        match RunitManager::control(&svc.name, act) {
                            Ok(msg) => self.statusbar.set_text(1, format!("{}: {}", svc.name, msg)),
                            Err(e) => self.statusbar.set_text(1, format!("sv error: {}", e)),
                        }
                        self.services = RunitManager::list_services();
                    }
                    return true;
                }
                if self.btn_svc_enable.take_click() {
                    if let Some(svc) = self.sel_svc.and_then(|i| self.services.get(i)) {
                        let src = Path::new("/etc/sv").join(&svc.name);
                        let dst = Path::new("/var/service").join(&svc.name);
                        let _ = std::os::unix::fs::symlink(src, dst);
                        self.services = RunitManager::list_services();
                    }
                    return true;
                }
                if self.btn_svc_disable.take_click() {
                    if let Some(svc) = self.sel_svc.and_then(|i| self.services.get(i)) {
                        let dst = Path::new("/var/service").join(&svc.name);
                        let _ = fs::remove_file(dst);
                        self.services = RunitManager::list_services();
                    }
                    return true;
                }
                let list_r = Rect::new(body.x + 12, body.y + 96, body.w - 24, body.h - 108);
                if let UiEvent::MouseDown { pos, button: 1 } = event {
                    if let Some(hit) = list_item_at(list_r, 24, self.svc_scroll, self.services.len(), pos) {
                        self.sel_svc = Some(hit);
                        return true;
                    }
                }
                if let UiEvent::Wheel { pos, delta } = event {
                    if list_r.contains(pos) {
                        if delta > 0 {
                            self.svc_scroll = self.svc_scroll.saturating_sub(2);
                        } else if self.svc_scroll + 1 < self.services.len() {
                            self.svc_scroll += 2;
                        }
                        return true;
                    }
                }
            }
            ControlSection::Desktop => {
                dirty |= self.btn_layout_float.on_event(event);
                dirty |= self.btn_layout_tile.on_event(event);
                dirty |= self.btn_launch_term.on_event(event);
                dirty |= self.btn_launch_files.on_event(event);
                dirty |= self.btn_launch_tasks.on_event(event);
                if self.btn_layout_float.take_click() {
                    let _ = fs::write("/run/space-layout", "float\n");
                    self.statusbar.set_text(1, "Window layout: Floating");
                    return true;
                }
                if self.btn_layout_tile.take_click() {
                    let _ = fs::write("/run/space-layout", "tile\n");
                    self.statusbar.set_text(1, "Window layout: Tiled");
                    return true;
                }
                if self.btn_launch_term.take_click() { let _ = Command::new("space-term").spawn(); return true; }
                if self.btn_launch_files.take_click() { let _ = Command::new("space-files").spawn(); return true; }
                if self.btn_launch_tasks.take_click() { let _ = Command::new("space-tasks").spawn(); return true; }
            }
        }

        dirty
    }
}

fn main() {
    let w = 780;
    let h = 520;
    let mut client = SpaceClient::connect("Control Panel", w, h).expect("connect failed");
    let mut app = ControlApp::new(w, h);
    client.run(&mut app);
}
