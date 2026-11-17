use space_core::font;
use space_core::{sys, Canvas, Color, Rect};
use space_ui::{draw_panel_3d, AppLogic, Lifecycle, Paintable, SpaceClient, Theme, UiEvent};
use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

const DEFAULT_COLS: usize = 80;
const DEFAULT_ROWS: usize = 28;
const CELL_W: i32 = 10;
const CELL_H: i32 = 17;
const PAD: i32 = 8;

#[derive(Clone, Copy)]
struct Cell {
    ch: char,
    fg: Color,
    bg: Color,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            fg: Color::rgb(230, 234, 240),
            bg: Color::rgb(16, 19, 24),
        }
    }
}

enum EscState {
    Normal,
    Esc,
    Csi(String),
}

struct TermApp {
    master_fd: c_int,
    child_pid: c_int,
    cols: usize,
    rows: usize,
    grid: Vec<Cell>,
    cur_r: usize,
    cur_c: usize,
    fg: Color,
    bg: Color,
    esc: EscState,
    utf8_buf: Vec<u8>,
}

impl TermApp {
    fn spawn(cols: usize, rows: usize) -> std::io::Result<Self> {
        let master_fd = unsafe { sys::posix_openpt(sys::O_RDWR | sys::O_NOCTTY | sys::O_CLOEXEC) };
        if master_fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        unsafe {
            sys::grantpt(master_fd);
            sys::unlockpt(master_fd);
        }
        let slave_name = sys::cstr_to_string(unsafe { sys::ptsname(master_fd) })
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "ptsname failed"))?;

        let ws = sys::WinSize {
            ws_row: rows as u16,
            ws_col: cols as u16,
            ws_xpixel: (cols as i32 * CELL_W) as u16,
            ws_ypixel: (rows as i32 * CELL_H) as u16,
        };
        unsafe { sys::ioctl(master_fd, sys::TIOCSWINSZ, &ws) };

        let pid = unsafe { sys::fork() };
        if pid < 0 {
            unsafe { sys::close(master_fd) };
            return Err(std::io::Error::last_os_error());
        }
        if pid == 0 {
            unsafe {
                sys::setsid();
                let sfd = sys::open_path(&slave_name, sys::O_RDWR, 0).unwrap_or(-1);
                if sfd >= 0 {
                    sys::ioctl(sfd, sys::TIOCSCTTY, 0);
                    sys::dup2(sfd, 0);
                    sys::dup2(sfd, 1);
                    sys::dup2(sfd, 2);
                    if sfd > 2 {
                        sys::close(sfd);
                    }
                }
                sys::close(master_fd);
                std::env::set_var("TERM", "xterm-256color");
                let shell = if std::path::Path::new("/bin/bash").exists() {
                    "/bin/bash"
                } else {
                    "/bin/sh"
                };
                let c_sh = CString::new(shell).unwrap();
                let argv = [c_sh.as_ptr(), ptr::null::<c_char>()];
                sys::execvp(c_sh.as_ptr(), argv.as_ptr());
                sys::_exit(127);
            }
        }

        let _ = sys::set_nonblocking(master_fd);
        Ok(Self {
            master_fd,
            child_pid: pid,
            cols,
            rows,
            grid: vec![Cell::default(); cols * rows],
            cur_r: 0,
            cur_c: 0,
            fg: Cell::default().fg,
            bg: Cell::default().bg,
            esc: EscState::Normal,
            utf8_buf: Vec::with_capacity(4),
        })
    }

    fn resize_grid(&mut self, new_cols: usize, new_rows: usize) {
        let new_cols = new_cols.max(10);
        let new_rows = new_rows.max(4);
        if new_cols == self.cols && new_rows == self.rows {
            return;
        }
        let mut next = vec![Cell::default(); new_cols * new_rows];
        let copy_rows = self.rows.min(new_rows);
        let copy_cols = self.cols.min(new_cols);
        for r in 0..copy_rows {
            let src_start = r * self.cols;
            let dst_start = r * new_cols;
            next[dst_start..dst_start + copy_cols]
                .copy_from_slice(&self.grid[src_start..src_start + copy_cols]);
        }
        self.cols = new_cols;
        self.rows = new_rows;
        self.grid = next;
        self.cur_r = self.cur_r.min(new_rows - 1);
        self.cur_c = self.cur_c.min(new_cols - 1);

        let ws = sys::WinSize {
            ws_row: new_rows as u16,
            ws_col: new_cols as u16,
            ws_xpixel: (new_cols as i32 * CELL_W) as u16,
            ws_ypixel: (new_rows as i32 * CELL_H) as u16,
        };
        unsafe { sys::ioctl(self.master_fd, sys::TIOCSWINSZ, &ws) };
    }

    fn scroll_up(&mut self) {
        if self.rows == 0 || self.cols == 0 {
            return;
        }
        self.grid.copy_within(self.cols.., 0);
        let start = (self.rows - 1) * self.cols;
        for c in &mut self.grid[start..] {
            *c = Cell::default();
        }
    }

    fn put_char(&mut self, ch: char) {
        match ch {
            '\r' => self.cur_c = 0,
            '\n' => {
                self.cur_r += 1;
                if self.cur_r >= self.rows {
                    self.scroll_up();
                    self.cur_r = self.rows - 1;
                }
            }
            '\x08' => {
                self.cur_c = self.cur_c.saturating_sub(1);
            }
            '\t' => {
                let next = (self.cur_c + 8) & !7;
                self.cur_c = next.min(self.cols - 1);
            }
            c if !c.is_control() => {
                if self.cur_c >= self.cols {
                    self.cur_c = 0;
                    self.cur_r += 1;
                    if self.cur_r >= self.rows {
                        self.scroll_up();
                        self.cur_r = self.rows - 1;
                    }
                }
                let idx = self.cur_r * self.cols + self.cur_c;
                if idx < self.grid.len() {
                    self.grid[idx] = Cell {
                        ch: c,
                        fg: self.fg,
                        bg: self.bg,
                    };
                }
                self.cur_c += 1;
            }
            _ => {}
        }
    }

    fn ansi_color(idx: u32, bright: bool) -> Color {
        match (idx, bright) {
            (0, false) => Color::rgb(16, 19, 24),
            (1, false) => Color::rgb(218, 68, 83),
            (2, false) => Color::rgb(39, 174, 96),
            (3, false) => Color::rgb(246, 116, 0),
            (4, false) => Color::rgb(61, 174, 233),
            (5, false) => Color::rgb(155, 89, 182),
            (6, false) => Color::rgb(26, 188, 156),
            (7, false) => Color::rgb(220, 224, 230),
            (0, true) => Color::rgb(90, 98, 112),
            (1, true) => Color::rgb(252, 92, 101),
            (2, true) => Color::rgb(46, 204, 113),
            (3, true) => Color::rgb(254, 211, 48),
            (4, true) => Color::rgb(75, 207, 250),
            (5, true) => Color::rgb(214, 162, 232),
            (6, true) => Color::rgb(85, 230, 193),
            _ => Color::WHITE,
        }
    }

    fn handle_csi(&mut self, seq: &str, cmd: char) {
        let is_private = seq.starts_with('?');
        let params: Vec<u32> = seq
            .trim_start_matches('?')
            .split(';')
            .filter_map(|s| if s.is_empty() { Some(0) } else { s.parse().ok() })
            .collect();
        if is_private {
            return;
        }
        match cmd {
            'J' => {
                let mode = params.first().copied().unwrap_or(0);
                if mode == 2 || mode == 3 {
                    self.grid.fill(Cell::default());
                    self.cur_r = 0;
                    self.cur_c = 0;
                } else if mode == 0 {
                    let start = (self.cur_r * self.cols + self.cur_c).min(self.grid.len());
                    self.grid[start..].fill(Cell::default());
                } else if mode == 1 {
                    let end = (self.cur_r * self.cols + self.cur_c + 1).min(self.grid.len());
                    self.grid[..end].fill(Cell::default());
                }
            }
            'K' => {
                let mode = params.first().copied().unwrap_or(0);
                let row_start = self.cur_r * self.cols;
                let row_end = ((self.cur_r + 1) * self.cols).min(self.grid.len());
                let cur_idx = (row_start + self.cur_c).min(row_end);
                match mode {
                    0 => self.grid[cur_idx..row_end].fill(Cell::default()),
                    1 => self.grid[row_start..cur_idx.min(row_end)].fill(Cell::default()),
                    2 => self.grid[row_start..row_end].fill(Cell::default()),
                    _ => {}
                }
            }
            'H' | 'f' => {
                let r = params.first().copied().unwrap_or(1).max(1) as usize - 1;
                let c = params.get(1).copied().unwrap_or(1).max(1) as usize - 1;
                self.cur_r = r.min(self.rows - 1);
                self.cur_c = c.min(self.cols - 1);
            }
            'G' => {
                let c = params.first().copied().unwrap_or(1).max(1) as usize - 1;
                self.cur_c = c.min(self.cols - 1);
            }
            'A' => self.cur_r = self.cur_r.saturating_sub(params.first().copied().unwrap_or(1).max(1) as usize),
            'B' => self.cur_r = (self.cur_r + params.first().copied().unwrap_or(1).max(1) as usize).min(self.rows - 1),
            'C' => self.cur_c = (self.cur_c + params.first().copied().unwrap_or(1).max(1) as usize).min(self.cols - 1),
            'D' => self.cur_c = self.cur_c.saturating_sub(params.first().copied().unwrap_or(1).max(1) as usize),
            'm' => {
                if params.is_empty() {
                    self.fg = Cell::default().fg;
                    self.bg = Cell::default().bg;
                    return;
                }
                for &p in &params {
                    match p {
                        0 => {
                            self.fg = Cell::default().fg;
                            self.bg = Cell::default().bg;
                        }
                        30..=37 => self.fg = Self::ansi_color(p - 30, false),
                        39 => self.fg = Cell::default().fg,
                        40..=47 => self.bg = Self::ansi_color(p - 40, false),
                        49 => self.bg = Cell::default().bg,
                        90..=97 => self.fg = Self::ansi_color(p - 90, true),
                        100..=107 => self.bg = Self::ansi_color(p - 100, true),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn feed_bytes(&mut self, data: &[u8]) {
        for &b in data {
            match &mut self.esc {
                EscState::Normal => {
                    if b == 0x1B {
                        self.utf8_buf.clear();
                        self.esc = EscState::Esc;
                    } else if b < 0x80 && self.utf8_buf.is_empty() {
                        self.put_char(b as char);
                    } else {
                        self.utf8_buf.push(b);
                        if let Ok(s) = std::str::from_utf8(&self.utf8_buf) {
                            let chars: Vec<char> = s.chars().collect();
                            self.utf8_buf.clear();
                            for c in chars {
                                self.put_char(c);
                            }
                        } else if self.utf8_buf.len() >= 4 {
                            self.utf8_buf.clear();
                        }
                    }
                }
                EscState::Esc => {
                    if b == b'[' {
                        self.esc = EscState::Csi(String::new());
                    } else {
                        self.esc = EscState::Normal;
                    }
                }
                EscState::Csi(buf) => {
                    if (0x40..=0x7E).contains(&b) {
                        let seq = std::mem::take(buf);
                        self.esc = EscState::Normal;
                        self.handle_csi(&seq, b as char);
                    } else {
                        buf.push(b as char);
                    }
                }
            }
        }
    }

    fn write_pty(&self, bytes: &[u8]) {
        unsafe {
            sys::write(self.master_fd, bytes.as_ptr() as *const c_void, bytes.len());
        }
    }
}

impl Drop for TermApp {
    fn drop(&mut self) {
        unsafe {
            sys::kill(self.child_pid, 15);
            sys::close(self.master_fd);
        }
    }
}

impl Lifecycle for TermApp {
    fn on_tick(&mut self) -> bool {
        let mut buf = [0u8; 4096];
        let mut got_any = false;
        loop {
            let n = unsafe { sys::read(self.master_fd, buf.as_mut_ptr() as *mut c_void, buf.len()) };
            if n <= 0 {
                break;
            }
            self.feed_bytes(&buf[..n as usize]);
            got_any = true;
        }
        got_any
    }
}

impl Paintable for TermApp {
    fn paint(&self, canvas: &mut dyn Canvas) {
        let b = canvas.bounds();
        let inner = Rect::new(b.x + 2, b.y + 2, (b.w - 4).max(1), (b.h - 4).max(1));
        canvas.fill_rect(inner, Color::rgb(16, 19, 24));
        draw_panel_3d(canvas, b, true, &Theme::PETRA_SLATE);
        let mut tmp = [0u8; 4];
        for r in 0..self.rows {
            let y = PAD + r as i32 * CELL_H;
            if y + CELL_H > b.h - 4 {
                break;
            }
            for c in 0..self.cols {
                let x = PAD + c as i32 * CELL_W;
                if x + CELL_W > b.w - 4 {
                    break;
                }
                let cell = self.grid[r * self.cols + c];
                if cell.bg != Color::rgb(16, 19, 24) {
                    canvas.fill_rect(Rect::new(x, y, CELL_W, CELL_H), cell.bg);
                }
                if cell.ch != ' ' {
                    let s = cell.ch.encode_utf8(&mut tmp);
                    font::draw_text_mono(canvas, x, y + 2, s, cell.fg, 2);
                }
            }
        }
        let cx = PAD + self.cur_c as i32 * CELL_W;
        let cy = PAD + self.cur_r as i32 * CELL_H;
        if cx + CELL_W <= b.w - 4 && cy + CELL_H <= b.h - 4 {
            canvas.fill_rect(
                Rect::new(cx, cy + 1, CELL_W - 1, CELL_H - 2),
                Color::argb(170, 61, 174, 233),
            );
        }
    }
}

impl AppLogic for TermApp {
    fn handle_event(&mut self, event: UiEvent) -> bool {
        if let UiEvent::Resize { width, height } = event {
            let new_cols = ((width as i32 - PAD * 2) / CELL_W).max(10) as usize;
            let new_rows = ((height as i32 - PAD * 2) / CELL_H).max(4) as usize;
            self.resize_grid(new_cols, new_rows);
            return true;
        }
        if let UiEvent::Key { code, pressed: true, ch, mods } = event {
            let ctrl = (mods & 2) != 0;
            if ctrl {
                if let Some(c) = ch {
                    let low = c.to_ascii_lowercase();
                    if ('a'..='z').contains(&low) {
                        let ctrl_byte = (low as u8 - b'a') + 1;
                        self.write_pty(&[ctrl_byte]);
                        return false;
                    }
                }
            }
            match code {
                14 => self.write_pty(b"\x7f"),
                15 => self.write_pty(b"\t"),
                28 => self.write_pty(b"\r"),
                103 => self.write_pty(b"\x1b[A"),
                108 => self.write_pty(b"\x1b[B"),
                106 => self.write_pty(b"\x1b[C"),
                105 => self.write_pty(b"\x1b[D"),
                _ => {
                    if let Some(c) = ch {
                        let mut buf = [0u8; 4];
                        let s = c.encode_utf8(&mut buf);
                        self.write_pty(s.as_bytes());
                    }
                }
            }
        }
        false
    }
}

fn main() {
    let w = (DEFAULT_COLS as i32 * CELL_W + PAD * 2) as u32;
    let h = (DEFAULT_ROWS as i32 * CELL_H + PAD * 2) as u32;
    let mut app = TermApp::spawn(DEFAULT_COLS, DEFAULT_ROWS).expect("pty spawn failed");
    let mut client = SpaceClient::connect("Space Terminal", w, h).expect("connect failed");
    client.run(&mut app);
}
