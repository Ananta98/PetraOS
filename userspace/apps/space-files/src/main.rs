use space_core::font;
use space_core::{Canvas, Rect};
use space_ui::{
    list_item_at, AppLogic, Button, EventListener, Lifecycle, Paintable, SpaceClient, Theme,
    UiEvent,
};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone)]
struct FileEntry {
    name: String,
    is_dir: bool,
    size: u64,
}

struct Pane {
    cwd: PathBuf,
    entries: Vec<FileEntry>,
    selected: Option<usize>,
    scroll: usize,
}

impl Pane {
    fn new(path: PathBuf) -> Self {
        let mut p = Self {
            cwd: path,
            entries: Vec::new(),
            selected: None,
            scroll: 0,
        };
        p.reload();
        p
    }

    fn reload(&mut self) {
        self.entries.clear();
        self.selected = None;
        self.scroll = 0;
        if let Ok(rd) = fs::read_dir(&self.cwd) {
            for ent in rd.flatten() {
                let name = ent.file_name().to_string_lossy().into_owned();
                let meta = ent.metadata();
                let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
                let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                self.entries.push(FileEntry { name, is_dir, size });
            }
        }
        self.entries
            .sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.cmp(&b.name)));
    }

    fn selected_path(&self) -> Option<PathBuf> {
        self.selected
            .and_then(|i| self.entries.get(i))
            .map(|e| self.cwd.join(&e.name))
    }
}

struct FilesApp {
    width: u32,
    height: u32,
    dual_pane: bool,
    active_pane: usize,
    panes: [Pane; 2],
    status: String,
    btn_up: Button,
    btn_dual: Button,
    btn_mkdir: Button,
    btn_copy: Button,
    btn_del: Button,
    btn_refresh: Button,
}

impl FilesApp {
    fn new(width: u32, height: u32) -> Self {
        let start = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        Self {
            width,
            height,
            dual_pane: true,
            active_pane: 0,
            panes: [Pane::new(start.clone()), Pane::new(PathBuf::from("/"))],
            status: String::from("Ready"),
            btn_up: Button::new(Rect::new(8, 8, 56, 28), "Up"),
            btn_dual: Button::new(Rect::new(72, 8, 108, 28), "Dual Pane"),
            btn_mkdir: Button::new(Rect::new(188, 8, 92, 28), "New Dir"),
            btn_copy: Button::new(Rect::new(288, 8, 76, 28), "Copy"),
            btn_del: Button::new(Rect::new(372, 8, 88, 28), "Delete"),
            btn_refresh: Button::new(Rect::new(468, 8, 96, 28), "Refresh"),
        }
    }

    fn pane_rects(&self) -> (Rect, Option<Rect>) {
        let top = 46;
        let h = self.height as i32 - top - 30;
        if self.dual_pane {
            let w = (self.width as i32 - 24) / 2;
            (
                Rect::new(8, top, w, h),
                Some(Rect::new(16 + w, top, w, h)),
            )
        } else {
            (Rect::new(8, top, self.width as i32 - 16, h), None)
        }
    }

    fn paint_pane(&self, canvas: &mut dyn Canvas, idx: usize, rect: Rect) {
        let t = Theme::BREEZE_DARK;
        let pane = &self.panes[idx];
        let active = self.active_pane == idx;
        canvas.fill_rounded_rect(rect, 6, t.bg_surface);
        canvas.draw_rect_outline(rect, 1, if active { t.accent } else { t.border });

        // Path header
        let hdr = Rect::new(rect.x, rect.y, rect.w, 28);
        canvas.fill_rect(hdr, t.bg_surface_alt);
        let path_str = pane.cwd.to_string_lossy();
        font::draw_text(canvas, hdr.x + 8, hdr.y + 6, &path_str, t.accent, 2);

        let list_r = Rect::new(rect.x + 4, rect.y + 32, rect.w - 8, rect.h - 36);
        let row_h = 24;
        let max_rows = (list_r.h / row_h).max(1) as usize;

        for (vi, ent) in pane.entries.iter().skip(pane.scroll).take(max_rows).enumerate() {
            let real_idx = pane.scroll + vi;
            let ry = list_r.y + (vi as i32) * row_h;
            let rr = Rect::new(list_r.x, ry, list_r.w, row_h - 2);
            if pane.selected == Some(real_idx) {
                canvas.fill_rounded_rect(rr, 4, t.accent_soft);
            }
            let tag = if ent.is_dir { "[D]" } else { "   " };
            let col = if ent.is_dir { t.accent } else { t.text_primary };
            font::draw_text(canvas, rr.x + 6, rr.y + 4, tag, col, 2);
            let name_short: String = ent.name.chars().take(28).collect();
            font::draw_text(canvas, rr.x + 48, rr.y + 4, &name_short, t.text_primary, 2);
            if !ent.is_dir {
                let sz = format!("{} B", ent.size);
                let sw = font::measure_text_width(&sz, 2);
                font::draw_text(canvas, rr.right() - sw - 8, rr.y + 4, &sz, t.text_secondary, 2);
            }
        }
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
}

impl Lifecycle for FilesApp {}

impl Paintable for FilesApp {
    fn paint(&self, canvas: &mut dyn Canvas) {
        let t = Theme::BREEZE_DARK;
        self.btn_up.paint(canvas);
        self.btn_dual.paint(canvas);
        self.btn_mkdir.paint(canvas);
        self.btn_copy.paint(canvas);
        self.btn_del.paint(canvas);
        self.btn_refresh.paint(canvas);

        let (r0, r1) = self.pane_rects();
        self.paint_pane(canvas, 0, r0);
        if let Some(r1) = r1 {
            self.paint_pane(canvas, 1, r1);
        }

        font::draw_text(canvas, 10, self.height as i32 - 22, &self.status, t.text_secondary, 2);
    }
}

impl AppLogic for FilesApp {
    fn handle_event(&mut self, event: UiEvent) -> bool {
        let mut dirty = false;
        dirty |= self.btn_up.on_event(event);
        dirty |= self.btn_dual.on_event(event);
        dirty |= self.btn_mkdir.on_event(event);
        dirty |= self.btn_copy.on_event(event);
        dirty |= self.btn_del.on_event(event);
        dirty |= self.btn_refresh.on_event(event);

        if self.btn_up.take_click() {
            let p = &mut self.panes[self.active_pane];
            if let Some(parent) = p.cwd.parent().map(|x| x.to_path_buf()) {
                p.cwd = parent;
                p.reload();
                self.status = format!("Opened {}", p.cwd.display());
            }
            return true;
        }
        if self.btn_dual.take_click() {
            self.dual_pane = !self.dual_pane;
            if !self.dual_pane {
                self.active_pane = 0;
            }
            return true;
        }
        if self.btn_refresh.take_click() {
            self.panes[0].reload();
            self.panes[1].reload();
            self.status = "Refreshed".to_string();
            return true;
        }
        if self.btn_mkdir.take_click() {
            let p = &mut self.panes[self.active_pane];
            let target = p.cwd.join(format!("new_dir_{}", p.entries.len() + 1));
            match fs::create_dir(&target) {
                Ok(_) => {
                    self.status = format!("Created {}", target.display());
                    p.reload();
                }
                Err(e) => self.status = format!("mkdir error: {}", e),
            }
            return true;
        }
        if self.btn_copy.take_click() {
            if let Some(src) = self.panes[self.active_pane].selected_path() {
                let dst_pane = if self.dual_pane { 1 - self.active_pane } else { self.active_pane };
                let name = src.file_name().unwrap_or_default().to_string_lossy().into_owned();
                let dst_name = if dst_pane == self.active_pane {
                    format!("{}.copy", name)
                } else {
                    name
                };
                let dst = self.panes[dst_pane].cwd.join(dst_name);
                match Self::copy_recursive(&src, &dst) {
                    Ok(_) => {
                        self.status = format!("Copied to {}", dst.display());
                        self.panes[dst_pane].reload();
                    }
                    Err(e) => self.status = format!("Copy error: {}", e),
                }
            }
            return true;
        }
        if self.btn_del.take_click() {
            if let Some(target) = self.panes[self.active_pane].selected_path() {
                let res = if target.is_dir() {
                    fs::remove_dir_all(&target)
                } else {
                    fs::remove_file(&target)
                };
                match res {
                    Ok(_) => {
                        self.status = format!("Deleted {}", target.display());
                        self.panes[self.active_pane].reload();
                    }
                    Err(e) => self.status = format!("Delete error: {}", e),
                }
            }
            return true;
        }

        let (r0, r1) = self.pane_rects();
        match event {
            UiEvent::MouseDown { pos, button: 1 } => {
                let target_idx = if r0.contains(pos) {
                    Some((0, r0))
                } else if let Some(r1) = r1 {
                    if r1.contains(pos) { Some((1, r1)) } else { None }
                } else {
                    None
                };

                if let Some((idx, rect)) = target_idx {
                    self.active_pane = idx;
                    let list_r = Rect::new(rect.x + 4, rect.y + 32, rect.w - 8, rect.h - 36);
                    let pane = &mut self.panes[idx];
                    if let Some(hit) = list_item_at(list_r, 24, pane.scroll, pane.entries.len(), pos) {
                        if pane.selected == Some(hit) && pane.entries[hit].is_dir {
                            pane.cwd = pane.cwd.join(&pane.entries[hit].name);
                            pane.reload();
                            self.status = format!("Opened {}", pane.cwd.display());
                        } else {
                            pane.selected = Some(hit);
                        }
                    }
                    return true;
                }
            }
            UiEvent::Wheel { pos, delta } => {
                let idx = if r1.map(|r| r.contains(pos)).unwrap_or(false) { 1 } else { 0 };
                let pane = &mut self.panes[idx];
                if delta > 0 {
                    pane.scroll = pane.scroll.saturating_sub(2);
                } else if delta < 0 && pane.scroll + 1 < pane.entries.len() {
                    pane.scroll += 2;
                }
                return true;
            }
            UiEvent::Key { code: 28, pressed: true, .. } => {
                let pane = &mut self.panes[self.active_pane];
                if let Some(sel) = pane.selected {
                    if let Some(ent) = pane.entries.get(sel) {
                        if ent.is_dir {
                            pane.cwd = pane.cwd.join(&ent.name);
                            pane.reload();
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
    let mut client = SpaceClient::connect("Space Files", 820, 520).expect("connect failed");
    let mut app = FilesApp::new(820, 520);
    client.run(&mut app);
}
