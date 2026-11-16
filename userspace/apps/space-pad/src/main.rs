use space_core::font;
use space_core::{Canvas, Color, Rect};
use space_ui::{
    AppLogic, Button, EventListener, Lifecycle, Paintable, SpaceClient, TextInput, Theme, UiEvent,
};
use std::fs;

struct DocTab {
    title: String,
    path: Option<String>,
    lines: Vec<String>,
    cur_line: usize,
    cur_col: usize,
    scroll: usize,
    modified: bool,
}

impl DocTab {
    fn new(title: &str) -> Self {
        Self {
            title: title.to_string(),
            path: None,
            lines: vec![String::new()],
            cur_line: 0,
            cur_col: 0,
            scroll: 0,
            modified: false,
        }
    }

    fn from_file(path: &str) -> std::io::Result<Self> {
        let content = fs::read_to_string(path)?;
        let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
        if lines.is_empty() {
            lines.push(String::new());
        }
        let title = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string());
        Ok(Self {
            title,
            path: Some(path.to_string()),
            lines,
            cur_line: 0,
            cur_col: 0,
            scroll: 0,
            modified: false,
        })
    }
}

struct PadApp {
    width: u32,
    height: u32,
    tabs: Vec<DocTab>,
    active_tab: usize,
    path_input: TextInput,
    btn_new: Button,
    btn_close: Button,
    btn_open: Button,
    btn_save: Button,
    status: String,
}

impl PadApp {
    fn new(width: u32, height: u32) -> Self {
        let mut first = DocTab::new("untitled.rs");
        first.lines = vec![
            "// Welcome to Space Pad".to_string(),
            "fn main() {".to_string(),
            "    println!(\"Hello PetraOS!\");".to_string(),
            "}".to_string(),
        ];
        Self {
            width,
            height,
            tabs: vec![first],
            active_tab: 0,
            path_input: TextInput::new(Rect::new(8, 8, 320, 28), "/path/to/file"),
            btn_new: Button::new(Rect::new(336, 8, 80, 28), "+ Tab"),
            btn_close: Button::new(Rect::new(424, 8, 88, 28), "Close"),
            btn_open: Button::new(Rect::new(520, 8, 76, 28), "Open"),
            btn_save: Button::new(Rect::new(604, 8, 76, 28), "Save"),
            status: String::from("Ready"),
        }
    }

    fn editor_rect(&self) -> Rect {
        Rect::new(8, 76, self.width as i32 - 16, self.height as i32 - 106)
    }

    fn is_keyword(tok: &str) -> bool {
        matches!(
            tok,
            "fn" | "let" | "mut" | "pub" | "struct" | "enum" | "impl" | "trait" | "use"
                | "if" | "else" | "for" | "while" | "loop" | "match" | "return" | "int"
                | "void" | "char" | "const" | "static" | "true" | "false"
        )
    }

    fn draw_highlighted_line(canvas: &mut dyn Canvas, x: i32, y: i32, line: &str) {
        let t = Theme::BREEZE_DARK;
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") || trimmed.starts_with('#') {
            font::draw_text(canvas, x, y, line, Color::rgb(112, 128, 144), 2);
            return;
        }
        let mut cx = x;
        let mut chars = line.char_indices().peekable();
        while let Some((idx, ch)) = chars.next() {
            if ch == '"' {
                let start = idx;
                let mut end = line.len();
                while let Some((j, c2)) = chars.next() {
                    if c2 == '"' {
                        end = j + 1;
                        break;
                    }
                }
                let seg = &line[start..end];
                font::draw_text(canvas, cx, y, seg, Color::rgb(152, 195, 121), 2);
                cx += font::measure_text_width(seg, 2);
            } else if ch.is_ascii_alphanumeric() || ch == '_' {
                let start = idx;
                let mut end = idx + ch.len_utf8();
                while let Some(&(j, c2)) = chars.peek() {
                    if c2.is_ascii_alphanumeric() || c2 == '_' {
                        end = j + c2.len_utf8();
                        chars.next();
                    } else {
                        break;
                    }
                }
                let word = &line[start..end];
                let col = if Self::is_keyword(word) {
                    t.accent
                } else if word.chars().all(|c| c.is_ascii_digit()) {
                    Color::rgb(209, 154, 102)
                } else {
                    t.text_primary
                };
                font::draw_text(canvas, cx, y, word, col, 2);
                cx += font::measure_text_width(word, 2);
            } else {
                let mut tmp = [0u8; 4];
                let s = ch.encode_utf8(&mut tmp);
                font::draw_text(canvas, cx, y, s, t.text_secondary, 2);
                cx += font::GLYPH_W * 2;
            }
        }
    }
}

impl Lifecycle for PadApp {}

impl Paintable for PadApp {
    fn paint(&self, canvas: &mut dyn Canvas) {
        let t = Theme::BREEZE_DARK;
        self.path_input.paint(canvas);
        self.btn_new.paint(canvas);
        self.btn_close.paint(canvas);
        self.btn_open.paint(canvas);
        self.btn_save.paint(canvas);

        // Tab bar
        for (i, tab) in self.tabs.iter().enumerate() {
            let tr = Rect::new(8 + i as i32 * 136, 42, 130, 28);
            let bg = if i == self.active_tab { t.accent_soft } else { t.bg_surface };
            canvas.fill_rounded_rect(tr, 4, bg);
            canvas.draw_rect_outline(tr, 1, if i == self.active_tab { t.accent } else { t.border });
            let lbl = if tab.modified {
                format!("*{}", tab.title)
            } else {
                tab.title.clone()
            };
            font::draw_text(canvas, tr.x + 8, tr.y + 6, &lbl.chars().take(10).collect::<String>(), t.text_primary, 2);
        }

        let er = self.editor_rect();
        canvas.fill_rounded_rect(er, 6, Color::rgb(18, 21, 27));
        canvas.draw_rect_outline(er, 1, t.border);

        let tab = &self.tabs[self.active_tab];
        let row_h = 20;
        let max_rows = (er.h / row_h).max(1) as usize;
        for (vi, line) in tab.lines.iter().skip(tab.scroll).take(max_rows).enumerate() {
            let real_l = tab.scroll + vi;
            let ry = er.y + 6 + vi as i32 * row_h;
            let lnum = format!("{:>3}", real_l + 1);
            font::draw_text(canvas, er.x + 6, ry, &lnum, t.text_secondary, 2);
            Self::draw_highlighted_line(canvas, er.x + 54, ry, line);

            if !self.path_input.focused && real_l == tab.cur_line {
                let cx = er.x + 54 + (tab.cur_col as i32) * font::GLYPH_W * 2;
                canvas.fill_rect(Rect::new(cx, ry, 2, 16), t.accent);
            }
        }

        font::draw_text(canvas, 10, self.height as i32 - 22, &self.status, t.text_secondary, 2);
    }
}

impl AppLogic for PadApp {
    fn handle_event(&mut self, event: UiEvent) -> bool {
        let mut dirty = false;
        dirty |= self.path_input.on_event(event);
        dirty |= self.btn_new.on_event(event);
        dirty |= self.btn_close.on_event(event);
        dirty |= self.btn_open.on_event(event);
        dirty |= self.btn_save.on_event(event);

        if self.btn_new.take_click() {
            let idx = self.tabs.len() + 1;
            self.tabs.push(DocTab::new(&format!("note_{}.txt", idx)));
            self.active_tab = self.tabs.len() - 1;
            return true;
        }
        if self.btn_close.take_click() && self.tabs.len() > 1 {
            self.tabs.remove(self.active_tab);
            self.active_tab = self.active_tab.min(self.tabs.len() - 1);
            return true;
        }
        if self.btn_open.take_click() {
            let p = self.path_input.text.trim().to_string();
            match DocTab::from_file(&p) {
                Ok(tab) => {
                    self.tabs.push(tab);
                    self.active_tab = self.tabs.len() - 1;
                    self.status = format!("Opened {}", p);
                }
                Err(e) => self.status = format!("Open failed: {}", e),
            }
            return true;
        }
        if self.btn_save.take_click() {
            let tab = &mut self.tabs[self.active_tab];
            let target = if !self.path_input.text.trim().is_empty() {
                self.path_input.text.trim().to_string()
            } else if let Some(ref p) = tab.path {
                p.clone()
            } else {
                format!("/tmp/{}", tab.title)
            };
            match fs::write(&target, tab.lines.join("\n")) {
                Ok(_) => {
                    tab.path = Some(target.clone());
                    tab.modified = false;
                    self.status = format!("Saved {}", target);
                }
                Err(e) => self.status = format!("Save failed: {}", e),
            }
            return true;
        }

        match event {
            UiEvent::MouseDown { pos, button: 1 } => {
                for i in 0..self.tabs.len() {
                    let tr = Rect::new(8 + i as i32 * 136, 42, 130, 28);
                    if tr.contains(pos) {
                        self.active_tab = i;
                        self.path_input.focused = false;
                        return true;
                    }
                }
                let er = self.editor_rect();
                if er.contains(pos) {
                    self.path_input.focused = false;
                    let tab = &mut self.tabs[self.active_tab];
                    let row = tab.scroll + ((pos.y - er.y - 6).max(0) / 20) as usize;
                    tab.cur_line = row.min(tab.lines.len().saturating_sub(1));
                    let col = ((pos.x - er.x - 54).max(0) / (font::GLYPH_W * 2)) as usize;
                    tab.cur_col = col.min(tab.lines[tab.cur_line].chars().count());
                    return true;
                }
            }
            UiEvent::Wheel { delta, .. } => {
                let tab = &mut self.tabs[self.active_tab];
                if delta > 0 {
                    tab.scroll = tab.scroll.saturating_sub(2);
                } else if delta < 0 && tab.scroll + 1 < tab.lines.len() {
                    tab.scroll += 2;
                }
                return true;
            }
            UiEvent::Key { code, pressed: true, ch, .. } if !self.path_input.focused => {
                let tab = &mut self.tabs[self.active_tab];
                match code {
                    103 => {
                        tab.cur_line = tab.cur_line.saturating_sub(1);
                        tab.cur_col = tab.cur_col.min(tab.lines[tab.cur_line].chars().count());
                    }
                    108 => {
                        if tab.cur_line + 1 < tab.lines.len() {
                            tab.cur_line += 1;
                            tab.cur_col = tab.cur_col.min(tab.lines[tab.cur_line].chars().count());
                        }
                    }
                    105 => tab.cur_col = tab.cur_col.saturating_sub(1),
                    106 => tab.cur_col = (tab.cur_col + 1).min(tab.lines[tab.cur_line].chars().count()),
                    14 => {
                        if tab.cur_col > 0 {
                            let line = &mut tab.lines[tab.cur_line];
                            let idx = line.char_indices().nth(tab.cur_col - 1).map(|(i, _)| i).unwrap_or(0);
                            line.remove(idx);
                            tab.cur_col -= 1;
                            tab.modified = true;
                        } else if tab.cur_line > 0 {
                            let cur = tab.lines.remove(tab.cur_line);
                            tab.cur_line -= 1;
                            tab.cur_col = tab.lines[tab.cur_line].chars().count();
                            tab.lines[tab.cur_line].push_str(&cur);
                            tab.modified = true;
                        }
                    }
                    28 => {
                        let line = &mut tab.lines[tab.cur_line];
                        let split_byte = line.char_indices().nth(tab.cur_col).map(|(i, _)| i).unwrap_or(line.len());
                        let rest = line.split_off(split_byte);
                        tab.cur_line += 1;
                        tab.cur_col = 0;
                        tab.lines.insert(tab.cur_line, rest);
                        tab.modified = true;
                    }
                    _ => {
                        if let Some(c) = ch {
                            if !c.is_control() {
                                let line = &mut tab.lines[tab.cur_line];
                                let ins_byte = line.char_indices().nth(tab.cur_col).map(|(i, _)| i).unwrap_or(line.len());
                                line.insert(ins_byte, c);
                                tab.cur_col += 1;
                                tab.modified = true;
                            }
                        }
                    }
                }
                return true;
            }
            _ => {}
        }
        dirty
    }
}

fn main() {
    let mut client = SpaceClient::connect("Space Pad", 760, 520).expect("connect failed");
    let mut app = PadApp::new(760, 520);
    client.run(&mut app);
}
