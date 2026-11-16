use space_core::font;
use space_core::{sys, Canvas, Color, Rect};
use space_ui::{
    draw_panel_3d, draw_progress_bar, list_item_at, AppLogic, Button, EventListener, Lifecycle,
    Paintable, SpaceClient, Theme, UiEvent,
};
use std::fs;

#[derive(Clone)]
struct ProcEntry {
    pid: i32,
    name: String,
    state: String,
    rss_kb: u64,
}

struct TasksApp {
    width: u32,
    height: u32,
    cpu_pct: f32,
    prev_total: u64,
    prev_idle: u64,
    mem_used_mb: u64,
    mem_total_mb: u64,
    procs: Vec<ProcEntry>,
    selected: Option<usize>,
    scroll: usize,
    tick: u64,
    status: String,
    btn_term: Button,
    btn_kill: Button,
    btn_stop: Button,
    btn_cont: Button,
    btn_refresh: Button,
}

impl TasksApp {
    fn new(width: u32, height: u32) -> Self {
        let mut app = Self {
            width,
            height,
            cpu_pct: 0.0,
            prev_total: 0,
            prev_idle: 0,
            mem_used_mb: 0,
            mem_total_mb: 0,
            procs: Vec::new(),
            selected: None,
            scroll: 0,
            tick: 0,
            status: String::from("Monitoring /proc"),
            btn_term: Button::new(Rect::new(8, 86, 108, 28), "SIGTERM"),
            btn_kill: Button::new(Rect::new(124, 86, 108, 28), "SIGKILL"),
            btn_stop: Button::new(Rect::new(240, 86, 108, 28), "SIGSTOP"),
            btn_cont: Button::new(Rect::new(356, 86, 108, 28), "SIGCONT"),
            btn_refresh: Button::new(Rect::new(472, 86, 100, 28), "Refresh"),
        };
        app.sample_proc();
        app
    }

    fn sample_proc(&mut self) {
        // CPU from /proc/stat
        if let Ok(stat) = fs::read_to_string("/proc/stat") {
            if let Some(line) = stat.lines().next() {
                let vals: Vec<u64> = line
                    .split_whitespace()
                    .skip(1)
                    .filter_map(|s| s.parse().ok())
                    .collect();
                if vals.len() >= 4 {
                    let idle = vals[3] + vals.get(4).copied().unwrap_or(0);
                    let total: u64 = vals.iter().sum();
                    if self.prev_total > 0 && total > self.prev_total {
                        let dt = total - self.prev_total;
                        let di = idle.saturating_sub(self.prev_idle);
                        self.cpu_pct = ((dt - di) as f32 / dt as f32) * 100.0;
                    }
                    self.prev_total = total;
                    self.prev_idle = idle;
                }
            }
        }

        // RAM from /proc/meminfo
        if let Ok(mem) = fs::read_to_string("/proc/meminfo") {
            let mut total = 0u64;
            let mut avail = 0u64;
            for line in mem.lines() {
                if let Some(r) = line.strip_prefix("MemTotal:") {
                    total = r.split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(0);
                } else if let Some(r) = line.strip_prefix("MemAvailable:") {
                    avail = r.split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(0);
                }
            }
            self.mem_total_mb = total / 1024;
            self.mem_used_mb = total.saturating_sub(avail) / 1024;
        }

        // Processes
        self.procs.clear();
        if let Ok(rd) = fs::read_dir("/proc") {
            for ent in rd.flatten() {
                let Ok(pid) = ent.file_name().to_string_lossy().parse::<i32>() else {
                    continue;
                };
                let name = fs::read_to_string(format!("/proc/{}/comm", pid))
                    .map(|s| s.trim().to_string())
                    .unwrap_or_else(|_| "?".to_string());
                let mut state = String::from("?");
                let mut rss_kb = 0u64;
                if let Ok(status) = fs::read_to_string(format!("/proc/{}/status", pid)) {
                    for line in status.lines() {
                        if let Some(r) = line.strip_prefix("State:") {
                            state = r.trim().chars().next().map(|c| c.to_string()).unwrap_or(state);
                        } else if let Some(r) = line.strip_prefix("VmRSS:") {
                            rss_kb = r.split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(0);
                        }
                    }
                }
                self.procs.push(ProcEntry {
                    pid,
                    name,
                    state,
                    rss_kb,
                });
            }
        }
        self.procs.sort_by(|a, b| b.rss_kb.cmp(&a.rss_kb).then_with(|| a.pid.cmp(&b.pid)));
    }

    fn list_rect(&self) -> Rect {
        Rect::new(8, 124, self.width as i32 - 16, self.height as i32 - 154)
    }

    fn send_sig(&mut self, sig: i32, label: &str) {
        if let Some(sel) = self.selected {
            if let Some(p) = self.procs.get(sel) {
                match sys::send_signal(p.pid, sig) {
                    Ok(_) => self.status = format!("Sent {} to PID {} ({})", label, p.pid, p.name),
                    Err(e) => self.status = format!("Signal error: {}", e),
                }
                self.sample_proc();
            }
        }
    }
}

impl Lifecycle for TasksApp {
    fn on_tick(&mut self) -> bool {
        self.tick += 1;
        if self.tick % 60 == 0 {
            self.sample_proc();
            return true;
        }
        false
    }
}

impl Paintable for TasksApp {
    fn paint(&self, canvas: &mut dyn Canvas) {
        let t = Theme::XP_LUNA;
        let half_w = (self.width as i32 - 24) / 2;
        draw_progress_bar(
            canvas,
            Rect::new(8, 12, half_w, 28),
            self.cpu_pct / 100.0,
            &format!("CPU: {:.1}%", self.cpu_pct),
            t.accent,
        );
        let mem_ratio = if self.mem_total_mb > 0 {
            self.mem_used_mb as f32 / self.mem_total_mb as f32
        } else {
            0.0
        };
        draw_progress_bar(
            canvas,
            Rect::new(16 + half_w, 12, half_w, 28),
            mem_ratio,
            &format!("RAM: {} / {} MB", self.mem_used_mb, self.mem_total_mb),
            t.success,
        );
        font::draw_text(
            canvas,
            10,
            52,
            &format!("Processes: {}", self.procs.len()),
            t.text_secondary,
            2,
        );

        self.btn_term.paint(canvas);
        self.btn_kill.paint(canvas);
        self.btn_stop.paint(canvas);
        self.btn_cont.paint(canvas);
        self.btn_refresh.paint(canvas);

        let lr = self.list_rect();
        canvas.fill_rect(lr, t.bg_surface);
        draw_panel_3d(canvas, lr, true, &t);
        canvas.draw_rect_outline(lr, 1, t.border);

        let hdr = Rect::new(lr.x + 2, lr.y + 2, lr.w - 4, 24);
        canvas.fill_rect(hdr, t.bg_window);
        font::draw_text(canvas, hdr.x + 8, hdr.y + 4, "PID     STATE  NAME                  RSS", t.accent, 2);

        let rows_r = Rect::new(lr.x + 4, lr.y + 28, lr.w - 8, lr.h - 32);
        let max_rows = (rows_r.h / 22).max(1) as usize;
        for (vi, p) in self.procs.iter().skip(self.scroll).take(max_rows).enumerate() {
            let real_idx = self.scroll + vi;
            let ry = rows_r.y + (vi as i32) * 22;
            let rr = Rect::new(rows_r.x, ry, rows_r.w, 20);
            let selected = self.selected == Some(real_idx);
            if selected {
                canvas.fill_rect(rr, t.bg_active);
            }
            let line = format!("{:<7} {:<6} {:<21} {} KB", p.pid, p.state, p.name.chars().take(20).collect::<String>(), p.rss_kb);
            font::draw_text(canvas, rr.x + 6, rr.y + 2, &line, if selected { Color::WHITE } else { t.text_primary }, 2);
        }

        font::draw_text(canvas, 10, self.height as i32 - 22, &self.status, t.text_secondary, 2);
    }
}

impl AppLogic for TasksApp {
    fn handle_event(&mut self, event: UiEvent) -> bool {
        let mut dirty = false;
        dirty |= self.btn_term.on_event(event);
        dirty |= self.btn_kill.on_event(event);
        dirty |= self.btn_stop.on_event(event);
        dirty |= self.btn_cont.on_event(event);
        dirty |= self.btn_refresh.on_event(event);

        if self.btn_term.take_click() {
            self.send_sig(15, "SIGTERM");
            return true;
        }
        if self.btn_kill.take_click() {
            self.send_sig(9, "SIGKILL");
            return true;
        }
        if self.btn_stop.take_click() {
            self.send_sig(19, "SIGSTOP");
            return true;
        }
        if self.btn_cont.take_click() {
            self.send_sig(18, "SIGCONT");
            return true;
        }
        if self.btn_refresh.take_click() {
            self.sample_proc();
            return true;
        }

        let lr = self.list_rect();
        let rows_r = Rect::new(lr.x + 4, lr.y + 26, lr.w - 8, lr.h - 30);
        match event {
            UiEvent::MouseDown { pos, button: 1 } => {
                if let Some(hit) = list_item_at(rows_r, 22, self.scroll, self.procs.len(), pos) {
                    self.selected = Some(hit);
                    return true;
                }
            }
            UiEvent::Wheel { delta, .. } => {
                if delta > 0 {
                    self.scroll = self.scroll.saturating_sub(3);
                } else if delta < 0 && self.scroll + 1 < self.procs.len() {
                    self.scroll += 3;
                }
                return true;
            }
            _ => {}
        }
        dirty
    }
}

fn main() {
    let mut client = SpaceClient::connect("Space Tasks", 720, 500).expect("connect failed");
    let mut app = TasksApp::new(720, 500);
    client.run(&mut app);
}
