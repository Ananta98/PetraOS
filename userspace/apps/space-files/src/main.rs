//! Explorer++ / ReactOS-inspired File Explorer in Petra Slate theme.

use space_core::font;
use space_core::icons::{self, IconId};
use space_core::{Canvas, Color, Rect};
use space_ui::{
    AppLogic, Button, EventListener, Layoutable, Lifecycle, Menu, MenuBar, MenuItem, Paintable,
    SpaceClient, Splitter, StatusBar, StylePainter, TextInput, Theme, ToolBar, ToolButton,
    UiEvent,
};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone)]
struct FileEntry {
    name: String,
    is_dir: bool,
    size: u64,
}

#[derive(Clone)]
struct BrowserTab {
    cwd: PathBuf,
    entries: Vec<FileEntry>,
    selected: Option<usize>,
    scroll: usize,
    back: Vec<PathBuf>,
    forward: Vec<PathBuf>,
    show_hidden: bool,
}

impl BrowserTab {
    fn new(path: PathBuf) -> Self {
        let mut t = Self {
            cwd: path,
            entries: Vec::new(),
            selected: None,
            scroll: 0,
            back: Vec::new(),
            forward: Vec::new(),
            show_hidden: false,
        };
        t.reload();
        t
    }

    fn title(&self) -> String {
        if self.cwd == Path::new("/") {
            String::from("System (/)")
        } else {
            self.cwd
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| self.cwd.to_string_lossy().into_owned())
        }
    }

    fn navigate(&mut self, path: PathBuf, record_history: bool) {
        if !path.is_dir() {
            return;
        }
        if record_history && self.cwd != path {
            self.back.push(self.cwd.clone());
            self.forward.clear();
        }
        self.cwd = path;
        self.reload();
    }

    fn reload(&mut self) {
        self.entries.clear();
        self.selected = None;
        self.scroll = 0;
        if let Ok(rd) = fs::read_dir(&self.cwd) {
            for ent in rd.flatten() {
                let name = ent.file_name().to_string_lossy().into_owned();
                if !self.show_hidden && name.starts_with('.') {
                    continue;
                }
                let meta = ent.metadata();
                let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
                let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                self.entries.push(FileEntry { name, is_dir, size });
            }
        }
        self.entries
            .sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    }

    fn selected_path(&self) -> Option<PathBuf> {
        self.selected
            .and_then(|i| self.entries.get(i))
            .map(|e| self.cwd.join(&e.name))
    }
}

#[derive(Clone)]
struct TreeNode {
    path: PathBuf,
    label: String,
    depth: usize,
    expanded: bool,
    has_children: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ViewMode {
    Icons,
    Details,
}

#[derive(Clone)]
enum ClipboardOp {
    Copy(PathBuf),
    Cut(PathBuf),
}

struct FilesApp {
    width: u32,
    height: u32,
    menubar: MenuBar,
    toolbar: ToolBar,
    address: TextInput,
    btn_go: Button,
    show_folders: bool,
    view_mode: ViewMode,
    splitter: Splitter,
    tabs: Vec<BrowserTab>,
    active_tab: usize,
    tree_nodes: Vec<TreeNode>,
    expanded_dirs: HashSet<PathBuf>,
    tree_scroll: usize,
    clipboard: Option<ClipboardOp>,
    statusbar: StatusBar,
    sys_summary: String,
    last_click: (Option<usize>, std::time::Instant),
}

impl FilesApp {
    fn new(width: u32, height: u32) -> Self {
        let start = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/root"));
        let start = if start.is_dir() { start } else { PathBuf::from("/") };

        let menubar = MenuBar::new(
            Rect::new(0, 0, 0, 0),
            vec![
                Menu::new(
                    "File",
                    vec![
                        MenuItem::action("new_tab", "New Tab", "Ctrl+T"),
                        MenuItem::action("close_tab", "Close Tab", "Ctrl+W"),
                        MenuItem::separator(),
                        MenuItem::action("mkdir", "New Folder", ""),
                        MenuItem::action("mkfile", "New Text File", ""),
                        MenuItem::separator(),
                        MenuItem::action("exit", "Exit", "Alt+F4"),
                    ],
                ),
                Menu::new(
                    "Edit",
                    vec![
                        MenuItem::action("cut", "Cut", "Ctrl+X"),
                        MenuItem::action("copy", "Copy", "Ctrl+C"),
                        MenuItem::action("paste", "Paste", "Ctrl+V"),
                        MenuItem::separator(),
                        MenuItem::action("delete", "Delete", "Del"),
                    ],
                ),
                Menu::new(
                    "View",
                    vec![
                        MenuItem::action("toggle_folders", "Folders Pane", ""),
                        MenuItem::action("view_icons", "Icons View", ""),
                        MenuItem::action("view_details", "Details View", ""),
                        MenuItem::action("toggle_hidden", "Show Hidden Files", ""),
                        MenuItem::separator(),
                        MenuItem::action("refresh", "Refresh", "F5"),
                    ],
                ),
                Menu::new(
                    "Go",
                    vec![
                        MenuItem::action("back", "Back", ""),
                        MenuItem::action("forward", "Forward", ""),
                        MenuItem::action("up", "Up One Level", ""),
                        MenuItem::action("go_root", "Computer (/)", ""),
                        MenuItem::action("go_home", "Home (/root)", ""),
                    ],
                ),
                Menu::new(
                    "Tools",
                    vec![
                        MenuItem::action("open_term", "Open Terminal", ""),
                    ],
                ),
            ],
        );

        let toolbar = ToolBar::new(
            Rect::new(0, 0, 0, 0),
            vec![
                ToolButton::new("back", "Back"),
                ToolButton::new("forward", "Fwd"),
                ToolButton::new("up", "Up"),
                ToolButton::new("toggle_folders", "Folders"),
                ToolButton::new("cut", "Cut"),
                ToolButton::new("copy", "Copy"),
                ToolButton::new("paste", "Paste"),
                ToolButton::new("delete", "Delete"),
                ToolButton::new("mkdir", "New Dir"),
                ToolButton::new("toggle_view", "View"),
                ToolButton::new("refresh", "Refresh"),
            ],
        );

        let mut expanded = HashSet::new();
        expanded.insert(PathBuf::from("/"));

        let tab0 = BrowserTab::new(start.clone());
        let mut address = TextInput::new(Rect::new(0, 0, 0, 0), "/");
        address.text = tab0.cwd.to_string_lossy().into_owned();

        let mut app = Self {
            width,
            height,
            menubar,
            toolbar,
            address,
            btn_go: Button::new(Rect::new(0, 0, 44, 24), "Go"),
            show_folders: true,
            view_mode: ViewMode::Icons,
            splitter: Splitter::new(Rect::new(0, 0, 0, 0), 0.25),
            tabs: vec![tab0],
            active_tab: 0,
            tree_nodes: Vec::new(),
            expanded_dirs: expanded,
            tree_scroll: 0,
            clipboard: None,
            statusbar: StatusBar::new(Rect::new(0, 0, 0, 0)),
            sys_summary: Self::read_sys_summary(),
            last_click: (None, std::time::Instant::now()),
        };
        app.rebuild_tree();
        app.relayout();
        app.update_status();
        app
    }

    fn read_sys_summary() -> String {
        let host = fs::read_to_string("/etc/hostname")
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| "petraos".to_string());
        let mem_mb = fs::read_to_string("/proc/meminfo")
            .ok()
            .and_then(|txt| {
                txt.lines()
                    .find(|l| l.starts_with("MemTotal:"))
                    .and_then(|l| l.split_whitespace().nth(1))
                    .and_then(|v| v.parse::<u64>().ok())
                    .map(|kb| kb / 1024)
            })
            .unwrap_or(0);
        format!("{}   |   Memory: {} MB   |   PetraOS x86_64", host, mem_mb)
    }

    fn tab(&self) -> &BrowserTab {
        &self.tabs[self.active_tab]
    }

    fn tab_mut(&mut self) -> &mut BrowserTab {
        &mut self.tabs[self.active_tab]
    }

    fn relayout(&mut self) {
        let w = self.width as i32;
        let h = self.height as i32;
        self.menubar.set_bounds(Rect::new(0, 0, w, 24));
        self.toolbar.set_bounds(Rect::new(0, 24, w, 32));
        // Address bar: y=56..86
        self.address.set_bounds(Rect::new(72, 59, (w - 128).max(100), 24));
        self.btn_go.set_bounds(Rect::new(w - 50, 59, 42, 24));
        // Drive bar: y=86..110
        // Main split area: y=112..h-78
        let main_h = (h - 112 - 78).max(120);
        self.splitter.set_bounds(Rect::new(6, 112, w - 12, main_h));
        // Bottom details pane: y=h-76..h-26 (50px)
        // Status bar: y=h-24..h
        self.statusbar.set_bounds(Rect::new(0, h - 24, w, 24));
    }

    fn update_status(&mut self) {
        let count = self.tab().entries.len();
        let cwd_str = self.tab().cwd.to_string_lossy().into_owned();
        self.address.text = cwd_str.clone();
        self.statusbar.set_text(0, format!("{} items", count));
        if let Some(sel) = self.tab().selected.and_then(|i| self.tab().entries.get(i)) {
            let kind = if sel.is_dir { "Folder" } else { "File" };
            self.statusbar.set_text(1, format!("{} ({}, {} B)", sel.name, kind, sel.size));
        } else {
            self.statusbar.set_text(1, "PetraOS VFS");
        }
        self.statusbar.set_text(2, cwd_str);
    }

    fn rebuild_tree(&mut self) {
        self.tree_nodes.clear();
        self.append_tree_dir(Path::new("/"), "Root (/)", 0);
    }

    fn append_tree_dir(&mut self, dir: &Path, label: &str, depth: usize) {
        let expanded = self.expanded_dirs.contains(dir);
        let mut subdirs = Vec::new();
        if let Ok(rd) = fs::read_dir(dir) {
            for ent in rd.flatten() {
                if ent.metadata().map(|m| m.is_dir()).unwrap_or(false) {
                    let name = ent.file_name().to_string_lossy().into_owned();
                    if !name.starts_with('.') {
                        subdirs.push((ent.path(), name));
                    }
                }
            }
        }
        subdirs.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
        self.tree_nodes.push(TreeNode {
            path: dir.to_path_buf(),
            label: label.to_string(),
            depth,
            expanded,
            has_children: !subdirs.is_empty(),
        });
        if expanded && depth < 5 {
            for (p, name) in subdirs.into_iter().take(64) {
                self.append_tree_dir(&p, &name, depth + 1);
            }
        }
    }

    fn navigate_to(&mut self, path: PathBuf) {
        if path.is_dir() {
            self.tab_mut().navigate(path.clone(), true);
            // Expand ancestor paths in tree
            let mut cur = path.as_path();
            while let Some(p) = cur.parent() {
                self.expanded_dirs.insert(p.to_path_buf());
                cur = p;
            }
            self.rebuild_tree();
            self.update_status();
        }
    }

    fn handle_action(&mut self, act: &str) -> bool {
        match act {
            "exit" => std::process::exit(0),
            "back" => {
                if let Some(prev) = self.tab_mut().back.pop() {
                    let cur = self.tab().cwd.clone();
                    self.tab_mut().forward.push(cur);
                    self.tab_mut().navigate(prev, false);
                    self.update_status();
                    return true;
                }
            }
            "forward" => {
                if let Some(next) = self.tab_mut().forward.pop() {
                    let cur = self.tab().cwd.clone();
                    self.tab_mut().back.push(cur);
                    self.tab_mut().navigate(next, false);
                    self.update_status();
                    return true;
                }
            }
            "up" => {
                if let Some(parent) = self.tab().cwd.parent().map(|p| p.to_path_buf()) {
                    self.navigate_to(parent);
                    return true;
                }
            }
            "go_root" => {
                self.navigate_to(PathBuf::from("/"));
                return true;
            }
            "go_home" => {
                let p = if Path::new("/root").is_dir() {
                    PathBuf::from("/root")
                } else {
                    PathBuf::from("/")
                };
                self.navigate_to(p);
                return true;
            }
            "toggle_folders" => {
                self.show_folders = !self.show_folders;
                return true;
            }
            "toggle_view" => {
                self.view_mode = match self.view_mode {
                    ViewMode::Icons => ViewMode::Details,
                    ViewMode::Details => ViewMode::Icons,
                };
                return true;
            }
            "view_icons" => {
                self.view_mode = ViewMode::Icons;
                return true;
            }
            "view_details" => {
                self.view_mode = ViewMode::Details;
                return true;
            }
            "toggle_hidden" => {
                let next = !self.tab().show_hidden;
                self.tab_mut().show_hidden = next;
                self.tab_mut().reload();
                self.update_status();
                return true;
            }
            "refresh" => {
                self.tab_mut().reload();
                self.rebuild_tree();
                self.update_status();
                return true;
            }
            "new_tab" => {
                let cwd = self.tab().cwd.clone();
                self.tabs.push(BrowserTab::new(cwd));
                self.active_tab = self.tabs.len() - 1;
                self.update_status();
                return true;
            }
            "close_tab" => {
                if self.tabs.len() > 1 {
                    self.tabs.remove(self.active_tab);
                    self.active_tab = self.active_tab.min(self.tabs.len() - 1);
                    self.update_status();
                    return true;
                }
            }
            "mkdir" => {
                let t = self.tab_mut();
                let target = t.cwd.join(format!("new_folder_{}", t.entries.len() + 1));
                if fs::create_dir(&target).is_ok() {
                    t.reload();
                    self.rebuild_tree();
                    self.update_status();
                    self.statusbar.set_text(2, format!("Created {}", target.display()));
                }
                return true;
            }
            "mkfile" => {
                let t = self.tab_mut();
                let target = t.cwd.join(format!("new_file_{}.txt", t.entries.len() + 1));
                if fs::write(&target, "").is_ok() {
                    t.reload();
                    self.update_status();
                    self.statusbar.set_text(2, format!("Created {}", target.display()));
                }
                return true;
            }
            "copy" => {
                if let Some(p) = self.tab().selected_path() {
                    self.clipboard = Some(ClipboardOp::Copy(p.clone()));
                    self.statusbar.set_text(2, format!("Copied {}", p.display()));
                    return true;
                }
            }
            "cut" => {
                if let Some(p) = self.tab().selected_path() {
                    self.clipboard = Some(ClipboardOp::Cut(p.clone()));
                    self.statusbar.set_text(2, format!("Cut {}", p.display()));
                    return true;
                }
            }
            "paste" => {
                if let Some(op) = self.clipboard.clone() {
                    let dst_dir = self.tab().cwd.clone();
                    match op {
                        ClipboardOp::Copy(src) => {
                            let name = src.file_name().unwrap_or_default().to_string_lossy().into_owned();
                            let mut dst = dst_dir.join(&name);
                            if dst == src {
                                dst = dst_dir.join(format!("{}.copy", name));
                            }
                            if Self::copy_recursive(&src, &dst).is_ok() {
                                self.tab_mut().reload();
                                self.rebuild_tree();
                                self.update_status();
                            }
                        }
                        ClipboardOp::Cut(src) => {
                            let name = src.file_name().unwrap_or_default().to_string_lossy().into_owned();
                            let dst = dst_dir.join(name);
                            if fs::rename(&src, &dst).is_ok() {
                                self.clipboard = None;
                                self.tab_mut().reload();
                                self.rebuild_tree();
                                self.update_status();
                            }
                        }
                    }
                    return true;
                }
            }
            "delete" => {
                if let Some(target) = self.tab().selected_path() {
                    let res = if target.is_dir() {
                        fs::remove_dir_all(&target)
                    } else {
                        fs::remove_file(&target)
                    };
                    if res.is_ok() {
                        self.tab_mut().reload();
                        self.rebuild_tree();
                        self.update_status();
                        self.statusbar.set_text(2, format!("Deleted {}", target.display()));
                    }
                    return true;
                }
            }
            "open_term" => {
                let _ = Command::new("space-term").current_dir(&self.tab().cwd).spawn();
                return true;
            }
            _ => {}
        }
        false
    }

    fn copy_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
        if src.is_dir() {
            fs::create_dir_all(dst)?;
            for ent in fs::read_dir(src)? {
                let ent = ent?;
                Self::copy_recursive(&ent.path(), &dst.join(ent.file_name()))?;
            }
        } else {
            fs::copy(src, dst)?;
        }
        Ok(())
    }

    fn drive_shortcuts() -> [(&'static str, &'static str); 6] {
        [
            ("/", "/"),
            ("Home", "/root"),
            ("etc", "/etc"),
            ("usr", "/usr"),
            ("tmp", "/tmp"),
            ("mnt", "/mnt"),
        ]
    }

    fn content_area_rect(&self) -> Rect {
        if self.show_folders {
            self.splitter.right_pane()
        } else {
            self.splitter.bounds()
        }
    }

    fn paint_tree(&self, canvas: &mut dyn Canvas, r: Rect, t: &Theme) {
        StylePainter::new(t).paint_sunken_field(canvas, r, false);
        let hdr = Rect::new(r.x + 1, r.y + 1, (r.w - 2).max(0), 22);
        canvas.fill_gradient_v(hdr, t.bg_surface, t.bg_window);
        canvas.fill_rect(Rect::new(hdr.x, hdr.bottom() - 1, hdr.w, 1), t.border);
        font::draw_text_bold(canvas, hdr.x + 8, hdr.y + 4, "Folders", t.text_primary, 2);
        // Close 'x' button on Folders header (Explorer++ style)
        let close_r = Rect::new(hdr.right() - 18, hdr.y + 3, 15, 15);
        canvas.draw_rect_outline(close_r, 1, t.border);
        font::draw_text(canvas, close_r.x + 4, close_r.y, "x", t.text_secondary, 1);

        let body = Rect::new(r.x + 2, hdr.bottom(), (r.w - 4).max(0), (r.h - 24).max(0));
        let row_h = 22;
        let max_rows = (body.h / row_h).max(1) as usize;
        let cwd = &self.tab().cwd;
        let folder_icon = IconId::new("files");
        let root_icon = IconId::new("drive");

        for (vi, node) in self.tree_nodes.iter().skip(self.tree_scroll).take(max_rows).enumerate() {
            let ry = body.y + vi as i32 * row_h;
            let row_r = Rect::new(body.x, ry, body.w, row_h);
            let selected = &node.path == cwd;
            if selected {
                canvas.fill_rect(row_r, t.bg_active);
            }
            let indent = (node.depth as i32) * 14 + 4;
            if node.has_children {
                let box_r = Rect::new(row_r.x + indent, ry + 5, 11, 11);
                canvas.fill_rect(box_r, Color::WHITE);
                canvas.draw_rect_outline(box_r, 1, t.border);
                let sym = if node.expanded { "-" } else { "+" };
                font::draw_text(canvas, box_r.x + 2, box_r.y - 1, sym, t.text_primary, 1);
            }
            let ic = if node.depth == 0 { &root_icon } else { &folder_icon };
            icons::draw_icon(canvas, row_r.x + indent + 15, ry + 3, 16, ic);
            let fg = if selected { Color::WHITE } else { t.text_primary };
            font::draw_text_clipped(canvas, row_r, row_r.x + indent + 35, ry + 4, &node.label, fg, 2);
        }
    }

    fn paint_tab_browser(&self, canvas: &mut dyn Canvas, r: Rect, t: &Theme) {
        // Tab bar at top of right pane (Explorer++ style)
        let tab_bar = Rect::new(r.x, r.y, r.w, 26);
        canvas.fill_rect(tab_bar, t.bg_window);
        canvas.fill_rect(Rect::new(tab_bar.x, tab_bar.bottom() - 1, tab_bar.w, 1), t.border);

        let mut tx = tab_bar.x + 4;
        for (i, tab) in self.tabs.iter().enumerate() {
            let title = tab.title();
            let tw = (font::measure_text_width(&title, 2) + 34).clamp(96, 170);
            if tx + tw > tab_bar.right() - 28 {
                break;
            }
            let tr = Rect::new(tx, tab_bar.y + 3, tw, 23);
            let active = i == self.active_tab;
            canvas.fill_rect(tr, if active { t.bg_surface } else { t.bg_surface_alt });
            canvas.draw_rect_outline(tr, 1, t.border);
            if active {
                canvas.fill_rect(Rect::new(tr.x + 1, tr.y, tr.w - 2, 2), t.accent);
                canvas.fill_rect(Rect::new(tr.x + 1, tr.bottom() - 1, tr.w - 2, 1), t.bg_surface);
            }
            icons::draw_icon(canvas, tr.x + 5, tr.y + 4, 14, &IconId::new("files"));
            font::draw_text_clipped(
                canvas,
                Rect::new(tr.x + 22, tr.y, tr.w - 36, tr.h),
                tr.x + 22,
                tr.y + 5,
                &title,
                t.text_primary,
                2,
            );
            if self.tabs.len() > 1 {
                font::draw_text(canvas, tr.right() - 12, tr.y + 4, "x", t.text_secondary, 1);
            }
            tx += tw + 3;
        }
        // '+' new tab button
        let add_r = Rect::new(tx + 2, tab_bar.y + 4, 22, 20);
        canvas.fill_rect(add_r, t.bg_surface_alt);
        canvas.draw_rect_outline(add_r, 1, t.border);
        font::draw_text(canvas, add_r.x + 6, add_r.y + 2, "+", t.text_primary, 2);

        // File content area
        let list_r = Rect::new(r.x, tab_bar.bottom(), r.w, (r.h - 26).max(40));
        StylePainter::new(t).paint_sunken_field(canvas, list_r, false);

        let tab = self.tab();
        let folder_icon = IconId::new("files");
        let file_icon = IconId::new("notepad");

        match self.view_mode {
            ViewMode::Icons => {
                let cell_w = 104;
                let cell_h = 74;
                let cols = ((list_r.w - 12) / cell_w).max(1) as usize;
                let max_rows = ((list_r.h - 12) / cell_h).max(1) as usize;
                let start = tab.scroll * cols;
                for (vi, ent) in tab.entries.iter().skip(start).take(cols * max_rows).enumerate() {
                    let real_idx = start + vi;
                    let col = (vi % cols) as i32;
                    let row = (vi / cols) as i32;
                    let cr = Rect::new(list_r.x + 6 + col * cell_w, list_r.y + 6 + row * cell_h, cell_w - 4, cell_h - 4);
                    let selected = tab.selected == Some(real_idx);
                    if selected {
                        canvas.fill_rounded_rect(cr, 2, t.accent_soft);
                        canvas.draw_rect_outline(cr, 1, t.accent);
                    }
                    let ic = if ent.is_dir { &folder_icon } else { &file_icon };
                    icons::draw_icon(canvas, cr.x + (cr.w - 32) / 2, cr.y + 6, 32, ic);
                    let short: String = ent.name.chars().take(12).collect();
                    let lw = font::measure_text_width(&short, 1);
                    let lx = cr.x + ((cr.w - lw) / 2).max(2);
                    let fg = if selected { t.accent } else { t.text_primary };
                    font::draw_text_clipped(canvas, cr, lx, cr.y + 44, &short, fg, 1);
                }
            }
            ViewMode::Details => {
                let hdr_r = Rect::new(list_r.x + 1, list_r.y + 1, list_r.w - 2, 22);
                canvas.fill_gradient_v(hdr_r, t.bg_surface, t.bg_window);
                canvas.fill_rect(Rect::new(hdr_r.x, hdr_r.bottom() - 1, hdr_r.w, 1), t.border);
                font::draw_text_bold(canvas, hdr_r.x + 28, hdr_r.y + 4, "Name", t.text_primary, 2);
                font::draw_text_bold(canvas, hdr_r.right() - 150, hdr_r.y + 4, "Type", t.text_primary, 2);
                font::draw_text_bold(canvas, hdr_r.right() - 74, hdr_r.y + 4, "Size", t.text_primary, 2);

                let rows_r = Rect::new(list_r.x + 2, hdr_r.bottom(), list_r.w - 4, (list_r.h - 24).max(20));
                let row_h = 22;
                let max_rows = (rows_r.h / row_h).max(1) as usize;
                for (vi, ent) in tab.entries.iter().skip(tab.scroll).take(max_rows).enumerate() {
                    let real_idx = tab.scroll + vi;
                    let rr = Rect::new(rows_r.x, rows_r.y + vi as i32 * row_h, rows_r.w, row_h);
                    let selected = tab.selected == Some(real_idx);
                    StylePainter::new(t).paint_row_bg(canvas, rr, real_idx, selected);
                    let ic = if ent.is_dir { &folder_icon } else { &file_icon };
                    icons::draw_icon(canvas, rr.x + 4, rr.y + 3, 16, ic);
                    let fg = if selected { Color::WHITE } else { t.text_primary };
                    let sub_fg = if selected { Color::WHITE } else { t.text_secondary };
                    font::draw_text_clipped(canvas, rr, rr.x + 26, rr.y + 4, &ent.name, fg, 2);
                    let kind = if ent.is_dir { "Folder" } else { "File" };
                    font::draw_text(canvas, rr.right() - 150, rr.y + 4, kind, sub_fg, 2);
                    if !ent.is_dir {
                        let sz = format!("{} B", ent.size);
                        font::draw_text(canvas, rr.right() - 74, rr.y + 4, &sz, sub_fg, 2);
                    }
                }
            }
        }
    }
}

impl Lifecycle for FilesApp {}

impl Paintable for FilesApp {
    fn paint(&self, canvas: &mut dyn Canvas) {
        let t = Theme::PETRA_SLATE;
        let w = self.width as i32;
        let h = self.height as i32;

        canvas.fill_rect(Rect::new(0, 0, w, h), t.bg_window);

        self.menubar.paint_with(canvas, &t);
        self.toolbar.paint_with(canvas, &t);

        // Address bar (Explorer++ style: "Address" + TextInput + Green "Go" button)
        let addr_strip = Rect::new(0, 56, w, 30);
        canvas.fill_rect(addr_strip, t.bg_window);
        font::draw_text(canvas, 8, 63, "Address", t.text_secondary, 2);
        self.address.paint_with(canvas, &t);
        let mut go_theme = t;
        go_theme.accent = t.success;
        let mut go_btn = self.btn_go.clone();
        go_btn.accent = true;
        go_btn.paint_with(canvas, &go_theme);
        canvas.fill_rect(Rect::new(0, 86, w, 1), t.bevel_shadow);

        // Quick Drive / Places strip (like C: D: E: in Explorer++)
        let drv_strip = Rect::new(0, 87, w, 24);
        canvas.fill_rect(drv_strip, t.bg_surface_alt);
        canvas.fill_rect(Rect::new(0, 110, w, 1), t.border);
        let mut dx = 8;
        let drive_icon = IconId::new("drive");
        for (lbl, target) in Self::drive_shortcuts() {
            let active = self.tab().cwd == Path::new(target);
            let bw = font::measure_text_width(lbl, 1) + 26;
            let dr = Rect::new(dx, 89, bw, 20);
            if active {
                canvas.fill_rect(dr, t.accent_soft);
                canvas.draw_rect_outline(dr, 1, t.accent);
            }
            icons::draw_icon(canvas, dr.x + 3, dr.y + 2, 14, &drive_icon);
            font::draw_text(canvas, dr.x + 20, dr.y + 4, lbl, t.text_primary, 1);
            dx += bw + 6;
        }

        // Split Folders tree + Tabbed browser
        if self.show_folders {
            self.paint_tree(canvas, self.splitter.left_pane(), &t);
            self.splitter.paint_with(canvas, &t);
        }
        self.paint_tab_browser(canvas, self.content_area_rect(), &t);

        // Explorer++ bottom gradient Details Pane (matching Image 4 in Petra Slate palette)
        let det_r = Rect::new(6, h - 76, w - 12, 48);
        canvas.fill_gradient_v(det_r, Theme::TITLE_ACTIVE_TOP, Theme::TITLE_ACTIVE_BOT);
        canvas.draw_rect_outline(det_r, 1, Theme::TITLE_ACTIVE_BORDER);
        let det_icon = if let Some(sel) = self.tab().selected.and_then(|i| self.tab().entries.get(i)) {
            if sel.is_dir { IconId::new("files") } else { IconId::new("notepad") }
        } else {
            IconId::new("my-computer")
        };
        icons::draw_icon(canvas, det_r.x + 12, det_r.y + 8, 32, &det_icon);
        if let Some(sel) = self.tab().selected.and_then(|i| self.tab().entries.get(i)) {
            font::draw_text_bold(canvas, det_r.x + 54, det_r.y + 8, &sel.name, Color::WHITE, 2);
            let sub = if sel.is_dir {
                format!("Directory in {}", self.tab().cwd.display())
            } else {
                format!("File Size: {} bytes   |   Location: {}", sel.size, self.tab().cwd.display())
            };
            font::draw_text(canvas, det_r.x + 54, det_r.y + 28, &sub, Color::rgb(0xD2, 0xE6, 0xFF), 1);
        } else {
            font::draw_text_bold(
                canvas,
                det_r.x + 54,
                det_r.y + 8,
                &format!("{}  ({} items)", self.tab().cwd.display(), self.tab().entries.len()),
                Color::WHITE,
                2,
            );
            font::draw_text(canvas, det_r.x + 54, det_r.y + 28, &self.sys_summary, Color::rgb(0xD2, 0xE6, 0xFF), 1);
        }

        self.statusbar.paint_with(canvas, &t);
        self.menubar.paint_popup(canvas);
    }
}

impl AppLogic for FilesApp {
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
            return self.handle_action(act) || dirty;
        }
        if menu_was_open {
            return dirty;
        }

        dirty |= self.toolbar.on_event(event);
        if let Some(act) = self.toolbar.take_action() {
            return self.handle_action(act) || dirty;
        }

        dirty |= self.address.on_event(event);
        if self.address.take_submit() {
            let p = PathBuf::from(self.address.text.trim());
            self.navigate_to(p);
            return true;
        }
        dirty |= self.btn_go.on_event(event);
        if self.btn_go.take_click() {
            let p = PathBuf::from(self.address.text.trim());
            self.navigate_to(p);
            return true;
        }

        if self.show_folders {
            dirty |= self.splitter.on_event(event);
        }

        let content_r = self.content_area_rect();
        match event {
            UiEvent::MouseDown { pos, button: 1 } => {
                // Quick drive bar
                if pos.y >= 87 && pos.y <= 110 {
                    let mut dx = 8;
                    for (lbl, target) in Self::drive_shortcuts() {
                        let bw = font::measure_text_width(lbl, 1) + 26;
                        let dr = Rect::new(dx, 89, bw, 20);
                        if dr.contains(pos) {
                            self.navigate_to(PathBuf::from(target));
                            return true;
                        }
                        dx += bw + 6;
                    }
                }

                // Folders tree click
                if self.show_folders && self.splitter.left_pane().contains(pos) {
                    let r = self.splitter.left_pane();
                    let close_r = Rect::new(r.right() - 19, r.y + 4, 15, 15);
                    if close_r.contains(pos) {
                        self.show_folders = false;
                        return true;
                    }
                    let body = Rect::new(r.x + 2, r.y + 23, (r.w - 4).max(0), (r.h - 24).max(0));
                    if body.contains(pos) {
                        let row = self.tree_scroll + ((pos.y - body.y) / 22) as usize;
                        if let Some(node) = self.tree_nodes.get(row).cloned() {
                            let indent = (node.depth as i32) * 14 + 4;
                            let toggle_r = Rect::new(body.x + indent - 2, body.y + (row - self.tree_scroll) as i32 * 22, 16, 22);
                            if node.has_children && toggle_r.contains(pos) {
                                if self.expanded_dirs.contains(&node.path) {
                                    self.expanded_dirs.remove(&node.path);
                                } else {
                                    self.expanded_dirs.insert(node.path.clone());
                                }
                                self.rebuild_tree();
                            } else {
                                self.navigate_to(node.path);
                            }
                            return true;
                        }
                    }
                }

                // Tab bar click
                let tab_bar = Rect::new(content_r.x, content_r.y, content_r.w, 26);
                if tab_bar.contains(pos) {
                    let mut tx = tab_bar.x + 4;
                    let mut hit_tab = None;
                    for (i, tab) in self.tabs.iter().enumerate() {
                        let tw = (font::measure_text_width(&tab.title(), 2) + 34).clamp(96, 170);
                        if tx + tw > tab_bar.right() - 28 {
                            break;
                        }
                        let tr = Rect::new(tx, tab_bar.y + 3, tw, 23);
                        if tr.contains(pos) {
                            let close_x = tr.right() - 14;
                            hit_tab = Some((i, self.tabs.len() > 1 && pos.x >= close_x));
                            break;
                        }
                        tx += tw + 3;
                    }
                    if let Some((i, close)) = hit_tab {
                        if close {
                            self.tabs.remove(i);
                            self.active_tab = self.active_tab.min(self.tabs.len() - 1);
                        } else {
                            self.active_tab = i;
                        }
                        self.update_status();
                        return true;
                    }
                    let add_r = Rect::new(tx + 2, tab_bar.y + 4, 22, 20);
                    if add_r.contains(pos) {
                        return self.handle_action("new_tab");
                    }
                }

                // File list click
                let list_r = Rect::new(content_r.x, content_r.y + 26, content_r.w, (content_r.h - 26).max(40));
                if list_r.contains(pos) {
                    let hit = match self.view_mode {
                        ViewMode::Icons => {
                            let cell_w = 104;
                            let cell_h = 74;
                            let cols = ((list_r.w - 12) / cell_w).max(1) as usize;
                            let col = (pos.x - (list_r.x + 6)) / cell_w;
                            let row = (pos.y - (list_r.y + 6)) / cell_h;
                            if col >= 0 && (col as usize) < cols && row >= 0 {
                                let idx = (self.tab().scroll + row as usize) * cols + col as usize;
                                if idx < self.tab().entries.len() { Some(idx) } else { None }
                            } else {
                                None
                            }
                        }
                        ViewMode::Details => {
                            let rows_y = list_r.y + 23;
                            if pos.y >= rows_y {
                                let idx = self.tab().scroll + ((pos.y - rows_y) / 22) as usize;
                                if idx < self.tab().entries.len() { Some(idx) } else { None }
                            } else {
                                None
                            }
                        }
                    };
                    if let Some(idx) = hit {
                        let now = std::time::Instant::now();
                        let dbl = self.last_click.0 == Some(idx)
                            && now.duration_since(self.last_click.1).as_millis() < 450;
                        self.last_click = (Some(idx), now);
                        self.tab_mut().selected = Some(idx);
                        if dbl {
                            let ent = self.tab().entries[idx].clone();
                            let target = self.tab().cwd.join(&ent.name);
                            if ent.is_dir {
                                self.navigate_to(target);
                            } else {
                                let _ = Command::new("space-pad").arg(&target).spawn();
                            }
                        } else {
                            self.update_status();
                        }
                    } else {
                        self.tab_mut().selected = None;
                        self.update_status();
                    }
                    return true;
                }
            }
            UiEvent::Wheel { pos, delta } => {
                if self.show_folders && self.splitter.left_pane().contains(pos) {
                    if delta > 0 {
                        self.tree_scroll = self.tree_scroll.saturating_sub(2);
                    } else if self.tree_scroll + 1 < self.tree_nodes.len() {
                        self.tree_scroll += 2;
                    }
                    return true;
                }
                if delta > 0 {
                    let s = self.tab().scroll.saturating_sub(1);
                    self.tab_mut().scroll = s;
                } else {
                    self.tab_mut().scroll += 1;
                }
                return true;
            }
            UiEvent::Key { code: 28, pressed: true, .. } if !self.address.focused => {
                if let Some(idx) = self.tab().selected {
                    if let Some(ent) = self.tab().entries.get(idx).cloned() {
                        let target = self.tab().cwd.join(&ent.name);
                        if ent.is_dir {
                            self.navigate_to(target);
                            return true;
                        }
                    }
                }
            }
            _ => {}
        }

        dirty
    }
}

fn main() {
    let w = 880;
    let h = 560;
    let mut client = SpaceClient::connect("File Explorer", w, h).expect("connect failed");
    let mut app = FilesApp::new(w, h);
    client.run(&mut app);
}
