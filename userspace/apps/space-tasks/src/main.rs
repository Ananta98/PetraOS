use space_core::{sys, Canvas, Rect};
use space_ui::{
    AppLogic, BoxLayout, EventListener, Insets, Layoutable, Lifecycle, Paintable, ProgressBar,
    SizePolicy, SpaceClient, StatusBar, TableColumn, TableView, Theme, ToolBar, ToolButton,
    UiEvent,
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
    tick: u64,
    cpu_bar: ProgressBar,
    mem_bar: ProgressBar,
    toolbar: ToolBar,
    table: TableView,
    statusbar: StatusBar,
}

impl TasksApp {
    fn new(width: u32, height: u32) -> Self {
        let toolbar = ToolBar::new(
            Rect::new(0, 0, 0, 0),
            vec![
                ToolButton::new("term", "SIGTERM").with_icon("stop"),
                ToolButton::new("kill", "SIGKILL").with_icon("delete"),
                ToolButton::new("stop", "SIGSTOP").with_icon("pause"),
                ToolButton::new("cont", "SIGCONT").with_icon("play"),
                ToolButton::new("refresh", "Refresh").with_icon("refresh"),
            ],
        );
        let table = TableView::new(
            Rect::new(0, 0, 0, 0),
            vec![
                TableColumn::new("PID", 76),
                TableColumn::new("State", 64),
                TableColumn::new("Process", 220),
                TableColumn::new("RSS", 110),
            ],
        );
        let statusbar = StatusBar::new(Rect::new(0, 0, 0, 0));
        let mut app = Self {
            width,
            height,
            cpu_pct: 0.0,
            prev_total: 0,
            prev_idle: 0,
            mem_used_mb: 0,
            mem_total_mb: 0,
            procs: Vec::new(),
            tick: 0,
            cpu_bar: ProgressBar::new(Rect::new(0, 0, 0, 0), 0.0, "CPU: 0.0%"),
            mem_bar: ProgressBar::new(Rect::new(0, 0, 0, 0), 0.0, "RAM: 0 / 0 MB")
                .with_fill(Theme::PETRA_SLATE.success),
            toolbar,
            table,
            statusbar,
        };
        app.relayout();
        app.sample_proc();
        app
    }

    fn relayout(&mut self) {
        let root = Rect::new(0, 0, self.width as i32, self.height as i32);
        let outer = BoxLayout::vbox(6, Insets::all(8));
        let slots = outer.compute(
            root,
            &[
                SizePolicy::Fixed(32), // CPU/RAM bars
                SizePolicy::Fixed(34), // ToolBar
                SizePolicy::Expand,    // TableView
                SizePolicy::Fixed(26), // StatusBar
            ],
        );
        let bar_slots = BoxLayout::hbox(8, Insets::ZERO)
            .compute(slots[0], &[SizePolicy::Expand, SizePolicy::Expand]);
        self.cpu_bar.set_bounds(bar_slots[0]);
        self.mem_bar.set_bounds(bar_slots[1]);
        self.toolbar.set_bounds(slots[1]);
        self.table.set_bounds(slots[2]);
        self.statusbar.set_bounds(slots[3]);
    }

    fn sample_proc(&mut self) {
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
                        self.cpu_pct = ((dt.saturating_sub(di)) as f32 * 100.0) / dt as f32;
                    }
                    self.prev_total = total;
                    self.prev_idle = idle;
                }
            }
        }

        if let Ok(mem) = fs::read_to_string("/proc/meminfo") {
            let mut total_kb = 0u64;
            let mut avail_kb = 0u64;
            for line in mem.lines() {
                if let Some(rest) = line.strip_prefix("MemTotal:") {
                    total_kb = rest.split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(0);
                } else if let Some(rest) = line.strip_prefix("MemAvailable:") {
                    avail_kb = rest.split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(0);
                }
            }
            self.mem_total_mb = total_kb / 1024;
            self.mem_used_mb = total_kb.saturating_sub(avail_kb) / 1024;
        }

        let prev_pid = self.table.selected.and_then(|i| self.procs.get(i)).map(|p| p.pid);
        self.procs.clear();
        if let Ok(rd) = fs::read_dir("/proc") {
            for ent in rd.flatten() {
                let fname = ent.file_name();
                let s = fname.to_string_lossy();
                if let Ok(pid) = s.parse::<i32>() {
                    let stat_path = format!("/proc/{}/stat", pid);
                    let name = fs::read_to_string(format!("/proc/{}/comm", pid))
                        .map(|c| c.trim().to_string())
                        .unwrap_or_else(|_| String::from("proc"));
                    let mut state = String::from("R");
                    if let Ok(st) = fs::read_to_string(stat_path) {
                        if let Some(rp) = st.rfind(')') {
                            if let Some(ch) = st[rp + 1..].split_whitespace().next() {
                                state = ch.to_string();
                            }
                        }
                    }
                    let mut rss_kb = 0u64;
                    if let Ok(status) = fs::read_to_string(format!("/proc/{}/status", pid)) {
                        for line in status.lines() {
                            if let Some(rest) = line.strip_prefix("VmRSS:") {
                                rss_kb = rest.split_whitespace().next().and_then(|v| v.parse().ok()).unwrap_or(0);
                            }
                        }
                    }
                    self.procs.push(ProcEntry { pid, name, state, rss_kb });
                }
            }
        }
        self.procs.sort_by_key(|p| p.pid);

        let rows: Vec<Vec<String>> = self
            .procs
            .iter()
            .map(|p| {
                vec![
                    p.pid.to_string(),
                    p.state.clone(),
                    p.name.clone(),
                    format!("{} KB", p.rss_kb),
                ]
            })
            .collect();
        self.table.set_rows(rows);
        self.table.selected = prev_pid.and_then(|pid| self.procs.iter().position(|p| p.pid == pid));
        self.cpu_bar.ratio = self.cpu_pct / 100.0;
        self.cpu_bar.label = format!("CPU: {:.1}%", self.cpu_pct);
        self.mem_bar.ratio = if self.mem_total_mb > 0 {
            self.mem_used_mb as f32 / self.mem_total_mb as f32
        } else {
            0.0
        };
        self.mem_bar.label = format!("RAM: {} / {} MB", self.mem_used_mb, self.mem_total_mb);
        self.statusbar.set_text(0, format!("Processes: {}", self.procs.len()));
        self.statusbar
            .set_text(1, format!("RAM: {} / {} MB", self.mem_used_mb, self.mem_total_mb));
    }

    fn send_sig(&mut self, sig: i32, label: &str) {
        if let Some(p) = self.table.selected.and_then(|i| self.procs.get(i)) {
            let pid = p.pid;
            let rc = unsafe { sys::kill(pid, sig) };
            if rc == 0 {
                self.statusbar.set_text(0, format!("Sent {} to PID {}", label, pid));
            } else {
                self.statusbar.set_text(0, format!("kill({}, {}) failed", pid, label));
            }
            self.sample_proc();
        } else {
            self.statusbar.set_text(0, "Select a process first");
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
        self.cpu_bar.paint(canvas);
        self.mem_bar.paint(canvas);
        self.toolbar.paint(canvas);
        self.table.paint(canvas);
        self.statusbar.paint(canvas);
    }
}

impl AppLogic for TasksApp {
    fn handle_event(&mut self, event: UiEvent) -> bool {
        if let UiEvent::Resize { width, height } = event {
            self.width = width;
            self.height = height;
            self.relayout();
            return true;
        }
        let mut dirty = false;
        dirty |= self.toolbar.on_event(event);
        dirty |= self.table.on_event(event);

        if let Some(act) = self.toolbar.take_action() {
            match act {
                "term" => self.send_sig(15, "SIGTERM"),
                "kill" => self.send_sig(9, "SIGKILL"),
                "stop" => self.send_sig(19, "SIGSTOP"),
                "cont" => self.send_sig(18, "SIGCONT"),
                "refresh" => {
                    self.sample_proc();
                    self.statusbar.set_text(0, "Refreshed /proc");
                }
                _ => {}
            }
            dirty = true;
        }
        dirty
    }
}

fn main() {
    let mut client = SpaceClient::connect("Task Manager", 560, 440).expect("connect space-tasks");
    let mut app = TasksApp::new(560, 440);
    client.run(&mut app);
}
