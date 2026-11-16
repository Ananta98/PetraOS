//! Raw `/dev/input/event*` evdev input driver (zero external crates).

use std::fs;
use std::io;
use std::os::raw::{c_int, c_ulong, c_void};

const EV_SYN: u16 = 0x00;
const EV_KEY: u16 = 0x01;
const EV_REL: u16 = 0x02;
const EV_ABS: u16 = 0x03;

const REL_X: u16 = 0x00;
const REL_Y: u16 = 0x01;
const REL_WHEEL: u16 = 0x08;

const ABS_X: u16 = 0x00;
const ABS_Y: u16 = 0x01;

pub const BTN_LEFT: u16 = 0x110;
pub const BTN_RIGHT: u16 = 0x111;
pub const BTN_MIDDLE: u16 = 0x112;

pub const KEY_ESC: u16 = 1;
pub const KEY_BACKSPACE: u16 = 14;
pub const KEY_TAB: u16 = 15;
pub const KEY_ENTER: u16 = 28;
pub const KEY_LEFTCTRL: u16 = 29;
pub const KEY_LEFTSHIFT: u16 = 42;
pub const KEY_RIGHTSHIFT: u16 = 54;
pub const KEY_LEFTALT: u16 = 56;
pub const KEY_RIGHTCTRL: u16 = 97;
pub const KEY_RIGHTALT: u16 = 100;
pub const KEY_UP: u16 = 103;
pub const KEY_LEFT: u16 = 105;
pub const KEY_RIGHT: u16 = 106;
pub const KEY_DOWN: u16 = 108;
pub const KEY_DELETE: u16 = 111;
pub const KEY_LEFTMETA: u16 = 125;
pub const KEY_RIGHTMETA: u16 = 126;

const EVIOCGABS_X: c_ulong = 0x8018_4540;
const EVIOCGABS_Y: c_ulong = 0x8018_4541;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
struct RawInputEvent {
    tv_sec: i64,
    tv_usec: i64,
    ty: u16,
    code: u16,
    value: i32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
struct InputAbsInfo {
    value: i32,
    minimum: i32,
    maximum: i32,
    fuzz: i32,
    flat: i32,
    resolution: i32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub super_key: bool,
}

impl Modifiers {
    pub fn to_bits(self) -> i32 {
        (self.shift as i32)
            | ((self.ctrl as i32) << 1)
            | ((self.alt as i32) << 2)
            | ((self.super_key as i32) << 3)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum InputEvent {
    PointerMoveRel { dx: i32, dy: i32 },
    PointerMoveAbs { x: i32, y: i32 },
    PointerButton { button: u16, pressed: bool },
    PointerWheel { delta: i32 },
    Key { code: u16, pressed: bool, ch: Option<char>, mods: Modifiers },
}

/// Input backend abstraction (OCP / DIP).
pub trait InputBackend {
    fn poll_events(&mut self, screen_w: i32, screen_h: i32, out: &mut Vec<InputEvent>);
}

struct EvdevDevice {
    fd: c_int,
    abs_max_x: i32,
    abs_max_y: i32,
}

pub struct EvdevInputBackend {
    devices: Vec<EvdevDevice>,
    mods: Modifiers,
}

impl EvdevInputBackend {
    pub fn open_all() -> io::Result<Self> {
        let mut devices = Vec::new();
        if let Ok(entries) = fs::read_dir("/dev/input") {
            let mut paths: Vec<String> = entries
                .flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    if name.starts_with("event") {
                        Some(format!("/dev/input/{}", name))
                    } else {
                        None
                    }
                })
                .collect();
            paths.sort();

            for path in paths {
                if let Ok(fd) = space_core::sys::open_path(
                    &path,
                    space_core::sys::O_RDONLY | space_core::sys::O_NONBLOCK | space_core::sys::O_CLOEXEC,
                    0,
                ) {
                    let mut abs_x = InputAbsInfo::default();
                    let mut abs_y = InputAbsInfo::default();
                    let max_x = if unsafe { space_core::sys::ioctl(fd, EVIOCGABS_X, &mut abs_x) } == 0
                        && abs_x.maximum > 0
                    {
                        abs_x.maximum
                    } else {
                        32767
                    };
                    let max_y = if unsafe { space_core::sys::ioctl(fd, EVIOCGABS_Y, &mut abs_y) } == 0
                        && abs_y.maximum > 0
                    {
                        abs_y.maximum
                    } else {
                        32767
                    };
                    devices.push(EvdevDevice {
                        fd,
                        abs_max_x: max_x,
                        abs_max_y: max_y,
                    });
                }
            }
        }
        Ok(Self {
            devices,
            mods: Modifiers::default(),
        })
    }

    pub fn modifiers(&self) -> Modifiers {
        self.mods
    }
}

impl InputBackend for EvdevInputBackend {
    fn poll_events(&mut self, screen_w: i32, screen_h: i32, out: &mut Vec<InputEvent>) {
        let mut buf = [RawInputEvent::default(); 32];
        let byte_len = std::mem::size_of_val(&buf);

        for dev in &self.devices {
            loop {
                let n = unsafe {
                    space_core::sys::read(dev.fd, buf.as_mut_ptr() as *mut c_void, byte_len)
                };
                if n <= 0 {
                    break;
                }
                let count = n as usize / std::mem::size_of::<RawInputEvent>();
                for ev in &buf[..count] {
                    match ev.ty {
                        EV_SYN => {}
                        EV_REL => match ev.code {
                            REL_X => out.push(InputEvent::PointerMoveRel { dx: ev.value, dy: 0 }),
                            REL_Y => out.push(InputEvent::PointerMoveRel { dx: 0, dy: ev.value }),
                            REL_WHEEL => out.push(InputEvent::PointerWheel { delta: ev.value }),
                            _ => {}
                        },
                        EV_ABS => match ev.code {
                            ABS_X => {
                                let x = (ev.value as i64 * screen_w as i64 / dev.abs_max_x.max(1) as i64) as i32;
                                out.push(InputEvent::PointerMoveAbs { x, y: -1 });
                            }
                            ABS_Y => {
                                let y = (ev.value as i64 * screen_h as i64 / dev.abs_max_y.max(1) as i64) as i32;
                                out.push(InputEvent::PointerMoveAbs { x: -1, y });
                            }
                            _ => {}
                        },
                        EV_KEY => {
                            let pressed = ev.value != 0;
                            match ev.code {
                                BTN_LEFT | BTN_RIGHT | BTN_MIDDLE => {
                                    out.push(InputEvent::PointerButton {
                                        button: ev.code,
                                        pressed,
                                    });
                                }
                                KEY_LEFTSHIFT | KEY_RIGHTSHIFT => self.mods.shift = pressed,
                                KEY_LEFTCTRL | KEY_RIGHTCTRL => self.mods.ctrl = pressed,
                                KEY_LEFTALT | KEY_RIGHTALT => self.mods.alt = pressed,
                                KEY_LEFTMETA | KEY_RIGHTMETA => self.mods.super_key = pressed,
                                _ => {
                                    let ch = keycode_to_ascii(ev.code, self.mods.shift);
                                    out.push(InputEvent::Key {
                                        code: ev.code,
                                        pressed,
                                        ch,
                                        mods: self.mods,
                                    });
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

impl Drop for EvdevInputBackend {
    fn drop(&mut self) {
        for dev in &self.devices {
            unsafe { space_core::sys::close(dev.fd) };
        }
    }
}

/// US QWERTY Linux evdev keycode -> ASCII mapping.
pub fn keycode_to_ascii(code: u16, shift: bool) -> Option<char> {
    let ch = match code {
        2 => if shift { '!' } else { '1' },
        3 => if shift { '@' } else { '2' },
        4 => if shift { '#' } else { '3' },
        5 => if shift { '$' } else { '4' },
        6 => if shift { '%' } else { '5' },
        7 => if shift { '^' } else { '6' },
        8 => if shift { '&' } else { '7' },
        9 => if shift { '*' } else { '8' },
        10 => if shift { '(' } else { '9' },
        11 => if shift { ')' } else { '0' },
        12 => if shift { '_' } else { '-' },
        13 => if shift { '+' } else { '=' },
        15 => '\t',
        16 => if shift { 'Q' } else { 'q' },
        17 => if shift { 'W' } else { 'w' },
        18 => if shift { 'E' } else { 'e' },
        19 => if shift { 'R' } else { 'r' },
        20 => if shift { 'T' } else { 't' },
        21 => if shift { 'Y' } else { 'y' },
        22 => if shift { 'U' } else { 'u' },
        23 => if shift { 'I' } else { 'i' },
        24 => if shift { 'O' } else { 'o' },
        25 => if shift { 'P' } else { 'p' },
        26 => if shift { '{' } else { '[' },
        27 => if shift { '}' } else { ']' },
        28 => '\n',
        30 => if shift { 'A' } else { 'a' },
        31 => if shift { 'S' } else { 's' },
        32 => if shift { 'D' } else { 'd' },
        33 => if shift { 'F' } else { 'f' },
        34 => if shift { 'G' } else { 'g' },
        35 => if shift { 'H' } else { 'h' },
        36 => if shift { 'J' } else { 'j' },
        37 => if shift { 'K' } else { 'k' },
        38 => if shift { 'L' } else { 'l' },
        39 => if shift { ':' } else { ';' },
        40 => if shift { '"' } else { '\'' },
        41 => if shift { '~' } else { '`' },
        43 => if shift { '|' } else { '\\' },
        44 => if shift { 'Z' } else { 'z' },
        45 => if shift { 'X' } else { 'x' },
        46 => if shift { 'C' } else { 'c' },
        47 => if shift { 'V' } else { 'v' },
        48 => if shift { 'B' } else { 'b' },
        49 => if shift { 'N' } else { 'n' },
        50 => if shift { 'M' } else { 'm' },
        51 => if shift { '<' } else { ',' },
        52 => if shift { '>' } else { '.' },
        53 => if shift { '?' } else { '/' },
        57 => ' ',
        _ => return None,
    };
    Some(ch)
}
