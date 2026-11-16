use space_core::font;
use space_core::{sys, Canvas, Color, Rect};
use space_ui::{draw_panel_3d, AppLogic, Lifecycle, Paintable, SpaceClient, Theme, UiEvent};
use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

const COLS: usize = 80;
const ROWS: usize = 28;
const CELL_W: i32 = 10;
const CELL_H: i32 = 17;

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
    grid: Vec<Cell>,
    cur_r: usize,
    cur_c: usize,
    fg: Color,
    bg: Color,
    esc: EscState,
}

impl TermApp {
    fn spawn() -> std::io::Result<Self> {
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
            ws_row: ROWS as u16,
            ws_col: COLS as u16,
            ws_xpixel: (COLS as i32 * CELL_W) as u16,
            ws_ypixel: (ROWS as i32 * CELL_H) as u16,
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
            grid: vec![Cell::default(); COLS * ROWS],
            cur_r: 0,
            cur_c: 0,
            fg: Cell::default().fg,
            bg: Cell::default().bg,
            esc: EscState::Normal,
        })
    }

    fn scroll_up(&mut self) {
        self.grid.copy_within(COLS.., 0);
        for c in &mut self.grid[(ROWS - 1) * COLS..] {
            *c = Cell::default();
        }
    }

    fn put_char(&mut self, ch: char) {
        match ch {
            '\r' => self.cur_c = 0,
            '\n' => {
                self.cur_r += 1;
                if self.cur_r >= ROWS {
                    self.scroll_up();
                    self.cur_r = ROWS - 1;
                }
            }
            '\x08' => {
                self.cur_c = self.cur_c.saturating_sub(1);
            }
            '\t' => {
                let next = (self.cur_c + 8) & !7;
                self.cur_c = next.min(COLS - 1);
            }
            c if !c.is_control() => {
                if self.cur_c >= COLS {
                    self.cur_c = 0;
                    self.cur_r += 1;
                    if self.cur_r >= ROWS {
                        self.scroll_up();
                        self.cur_r = ROWS - 1;
                    }
                }
                self.grid[self.cur_r * COLS + self.cur_c] = Cell {
                    ch: c,
                    fg: self.fg,
                    bg: self.bg,
                };
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
        let params: Vec<u32> = seq
            .trim_start_matches('?')
            .split(';')
            .filter_map(|s| if s.is_empty() { Some(0) } else { s.parse().ok() })
            .collect();
        match cmd {
            'J' => {
                let mode = params.first().copied().unwrap_or(0);
                if mode == 2 || mode == 3 {
                    self.grid.fill(Cell::default());
                    self.cur_r = 0;
                    self.cur_c = 0;
                }
            }
            'K' => {
                let start = self.cur_r * COLS + self.cur_c;
                let end = ((self.cur_r + 1) * COLS).min(self.grid.len());
                for c in &mut self.grid[start..end] {
                    *c = Cell::default();
                }
            }
            'H' | 'f' => {
                let r = params.first().copied().unwrap_or(1).max(1) as usize - 1;
                let c = params.get(1).copied().unwrap_or(1).max(1) as usize - 1;
                self.cur_r = r.min(ROWS - 1);
                self.cur_c = c.min(COLS - 1);
            }
            'A' => self.cur_r = self.cur_r.saturating_sub(params.first().copied().unwrap_or(1).max(1) as usize),
            'B' => self.cur_r = (self.cur_r + params.first().copied().unwrap_or(1).max(1) as usize).min(ROWS - 1),
            'C' => self.cur_c = (self.cur_c + params.first().copied().unwrap_or(1).max(1) as usize).min(COLS - 1),
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
            let ch = b as char;
            match &mut self.esc {
                EscState::Normal => {
                    if b == 0x1B {
                        self.esc = EscState::Esc;
                    } else {
                        self.put_char(ch);
                    }
                }
                EscState::Esc => {
                    if ch == '[' {
                        self.esc = EscState::Csi(String::new());
                    } else {
                        self.esc = EscState::Normal;
                    }
                }
                EscState::Csi(buf) => {
                    if (0x40..=0x7E).contains(&b) {
                        let seq = std::mem::take(buf);
                        self.esc = EscState::Normal;
                        self.handle_csi(&seq, ch);
                    } else {
                        buf.push(ch);
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
        let inner = Rect::new(b.x + 3, b.y + 3, (b.w - 6).max(1), (b.h - 6).max(1));
        canvas.fill_rect(inner, Color::rgb(16, 19, 24));
        draw_panel_3d(canvas, b, true, &Theme::XP_LUNA);
        let mut tmp = [0u8; 4];
        for r in 0..ROWS {
            for c in 0..COLS {
                let cell = self.grid[r * COLS + c];
                let x = 8 + c as i32 * CELL_W;
                let y = 8 + r as i32 * CELL_H;
                if cell.bg != Color::rgb(16, 19, 24) {
                    canvas.fill_rect(Rect::new(x, y, CELL_W, CELL_H), cell.bg);
                }
                if cell.ch != ' ' {
                    let s = cell.ch.encode_utf8(&mut tmp);
                    font::draw_text_mono(canvas, x, y + 2, s, cell.fg, 2);
                }
            }
        }
        // Cursor block
        let cx = 8 + self.cur_c as i32 * CELL_W;
        let cy = 8 + self.cur_r as i32 * CELL_H;
        canvas.fill_rect(Rect::new(cx, cy + 1, CELL_W - 1, CELL_H - 2), Color::argb(170, 61, 174, 233));
    }
}

impl AppLogic for TermApp {
    fn handle_event(&mut self, event: UiEvent) -> bool {
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
    let w = (COLS as i32 * CELL_W + 16) as u32;
    let h = (ROWS as i32 * CELL_H + 16) as u32;
    let mut app = TermApp::spawn().expect("pty spawn failed");
    let mut client = SpaceClient::connect("Space Terminal", w, h).expect("connect failed");
    client.run(&mut app);
}
