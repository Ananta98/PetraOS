// SPDX-License-Identifier: GPL-2.0

//! Input event ABI (`linux/input.h`, `linux/input-event-codes.h`).
//!
//! This is the evdev contract: the `struct input_event` layout, the event
//! types and codes carried in it, the device identification block and the
//! capability ioctls. Values are the Linux x86-64 ones so that an unmodified
//! userspace input stack (libinput, SDL, ...) reads them unchanged.
//!
//! | Item | Section of `linux/input.h` |
//! |---|---|
//! | [`InputEvent`], [`InputId`] | `struct input_event`, `struct input_id` |
//! | [`ev`] | `EV_*` event types and `SYN_*` codes |
//! | [`key`] | `KEY_*` / `BTN_*` key and button codes |
//! | [`rel`], [`abs`] | `REL_*`, `ABS_*` axis codes |
//! | [`led`], [`msc`], [`snd`], [`sw`] | the remaining event classes |
//! | [`ioctl`] | `EVIOCG*`, `EVIOCS*` and `UI_*` request numbers |

use core::mem::size_of;

use crate::time::Timeval;

/// `KEY_MAX`, the highest key/button code the bitmaps must cover.
pub const KEY_MAX: usize = 0x2ff;
/// `REL_MAX`.
pub const REL_MAX: usize = 0x0f;
/// `ABS_MAX`.
pub const ABS_MAX: usize = 0x3f;
/// `LED_MAX`.
pub const LED_MAX: usize = 0x0f;
/// `MSC_MAX`.
pub const MSC_MAX: usize = 0x07;
/// `SND_MAX`.
pub const SND_MAX: usize = 0x07;
/// `SW_MAX`.
pub const SW_MAX: usize = 0x0f;
/// `INPUT_PROP_MAX`.
pub const INPUT_PROP_MAX: usize = 0x1f;

/// Bytes needed for a capability bitmap covering codes `0..=max`.
pub const fn bitmap_len(max: usize) -> usize {
    (max + 8) / 8
}

/// Absolute axis information (`struct input_absinfo`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputAbsInfo {
    pub value: i32,
    pub minimum: i32,
    pub maximum: i32,
    pub fuzz: i32,
    pub flat: i32,
    pub resolution: i32,
}

impl InputAbsInfo {
    pub const SIZE: usize = size_of::<Self>();

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut buf = [0u8; Self::SIZE];
        buf[0..4].copy_from_slice(&self.value.to_ne_bytes());
        buf[4..8].copy_from_slice(&self.minimum.to_ne_bytes());
        buf[8..12].copy_from_slice(&self.maximum.to_ne_bytes());
        buf[12..16].copy_from_slice(&self.fuzz.to_ne_bytes());
        buf[16..20].copy_from_slice(&self.flat.to_ne_bytes());
        buf[20..24].copy_from_slice(&self.resolution.to_ne_bytes());
        buf
    }

    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < Self::SIZE {
            return None;
        }
        Some(Self {
            value: i32::from_ne_bytes(buf[0..4].try_into().ok()?),
            minimum: i32::from_ne_bytes(buf[4..8].try_into().ok()?),
            maximum: i32::from_ne_bytes(buf[8..12].try_into().ok()?),
            fuzz: i32::from_ne_bytes(buf[12..16].try_into().ok()?),
            flat: i32::from_ne_bytes(buf[16..20].try_into().ok()?),
            resolution: i32::from_ne_bytes(buf[20..24].try_into().ok()?),
        })
    }
}

/// Device properties (`INPUT_PROP_*`).
pub mod prop {
    pub const POINTER: u16 = 0x00;
    pub const DIRECT: u16 = 0x01;
    pub const BUTTONPAD: u16 = 0x02;
    pub const SEMI_MT: u16 = 0x03;
    pub const TOPBUTTONPAD: u16 = 0x04;
    pub const POINTING_STICK: u16 = 0x05;
    pub const ACCELEROMETER: u16 = 0x06;
    pub const MAX: u16 = 0x1f;
}

/// Auto-repeat codes (`REP_*`).
pub mod rep {
    pub const DELAY: usize = 0;
    pub const PERIOD: usize = 1;
    pub const MAX: usize = 0x01;
}

/// Clock IDs for `EVIOCSCLOCKID`.
pub const CLOCK_REALTIME: i32 = 0;
pub const CLOCK_MONOTONIC: i32 = 1;
pub const CLOCK_BOOTTIME: i32 = 7;


/// Identifies the device to userspace (`struct input_id`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputId {
    /// [`bus`] type constant.
    pub bustype: u16,
    pub vendor: u16,
    pub product: u16,
    pub version: u16,
}

impl InputId {
    /// The `input_id` block of a `uinput_user_dev` description.
    pub const SIZE: usize = size_of::<Self>();

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut buf = [0u8; Self::SIZE];
        buf[0..2].copy_from_slice(&self.bustype.to_ne_bytes());
        buf[2..4].copy_from_slice(&self.vendor.to_ne_bytes());
        buf[4..6].copy_from_slice(&self.product.to_ne_bytes());
        buf[6..8].copy_from_slice(&self.version.to_ne_bytes());
        buf
    }

    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < Self::SIZE {
            return None;
        }
        Some(Self {
            bustype: u16::from_ne_bytes(buf[0..2].try_into().ok()?),
            vendor: u16::from_ne_bytes(buf[2..4].try_into().ok()?),
            product: u16::from_ne_bytes(buf[4..6].try_into().ok()?),
            version: u16::from_ne_bytes(buf[6..8].try_into().ok()?),
        })
    }
}

/// A single event delivered by `/dev/input/eventN` (`struct input_event`).
///
/// 24 bytes on x86-64, matching Linux exactly: a `timeval`, the type/code
/// pair and a signed value. A report ends with `EV_SYN`/`SYN_REPORT`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputEvent {
    /// Timestamp of the event.
    pub time: Timeval,
    /// One of the `EV_*` types in [`ev`].
    pub type_: u16,
    /// Code within `type_` (a [`key`], [`rel`], ... code).
    pub code: u16,
    /// State: 1/0 for keys, a signed delta for relative axes.
    pub value: i32,
}

impl InputEvent {
    /// Wire size of one event.
    pub const SIZE: usize = 24;

    pub const fn new(type_: u16, code: u16, value: i32) -> Self {
        Self {
            time: Timeval::ZERO,
            type_,
            code,
            value,
        }
    }

    /// Returns the event stamped with the current wall-clock time.
    pub fn now(type_: u16, code: u16, value: i32) -> Self {
        Self {
            time: crate::time::get_timeval(),
            type_,
            code,
            value,
        }
    }

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut buf = [0u8; Self::SIZE];
        buf[0..16].copy_from_slice(&self.time.to_bytes());
        buf[16..18].copy_from_slice(&self.type_.to_ne_bytes());
        buf[18..20].copy_from_slice(&self.code.to_ne_bytes());
        buf[20..24].copy_from_slice(&self.value.to_ne_bytes());
        buf
    }
}

/// Event types (`EV_*`).
pub mod ev {
    pub const SYN: u16 = 0x00;
    pub const KEY: u16 = 0x01;
    pub const REL: u16 = 0x02;
    pub const ABS: u16 = 0x03;
    pub const MSC: u16 = 0x04;
    pub const SW: u16 = 0x05;
    pub const LED: u16 = 0x11;
    pub const SND: u16 = 0x12;
    pub const REP: u16 = 0x14;
    pub const FF: u16 = 0x15;
    pub const PWR: u16 = 0x16;
    pub const FF_STATUS: u16 = 0x17;
    pub const MAX: u16 = 0x1f;

    pub const SYN_REPORT: u16 = 0;
    pub const SYN_CONFIG: u16 = 1;
    pub const SYN_MT_REPORT: u16 = 2;
    pub const SYN_DROPPED: u16 = 3;
}

/// Key and button codes (`KEY_*`, `BTN_*`).
pub mod key {
    pub const RESERVED: u16 = 0;
    pub const ESC: u16 = 1;
    pub const NUM1: u16 = 2;
    pub const NUM2: u16 = 3;
    pub const NUM3: u16 = 4;
    pub const NUM4: u16 = 5;
    pub const NUM5: u16 = 6;
    pub const NUM6: u16 = 7;
    pub const NUM7: u16 = 8;
    pub const NUM8: u16 = 9;
    pub const NUM9: u16 = 10;
    pub const NUM0: u16 = 11;
    pub const MINUS: u16 = 12;
    pub const EQUAL: u16 = 13;
    pub const BACKSPACE: u16 = 14;
    pub const TAB: u16 = 15;
    pub const Q: u16 = 16;
    pub const W: u16 = 17;
    pub const E: u16 = 18;
    pub const R: u16 = 19;
    pub const T: u16 = 20;
    pub const Y: u16 = 21;
    pub const U: u16 = 22;
    pub const I: u16 = 23;
    pub const O: u16 = 24;
    pub const P: u16 = 25;
    pub const LEFTBRACE: u16 = 26;
    pub const RIGHTBRACE: u16 = 27;
    pub const ENTER: u16 = 28;
    pub const LEFTCTRL: u16 = 29;
    pub const A: u16 = 30;
    pub const S: u16 = 31;
    pub const D: u16 = 32;
    pub const F: u16 = 33;
    pub const G: u16 = 34;
    pub const H: u16 = 35;
    pub const J: u16 = 36;
    pub const K: u16 = 37;
    pub const L: u16 = 38;
    pub const SEMICOLON: u16 = 39;
    pub const APOSTROPHE: u16 = 40;
    pub const GRAVE: u16 = 41;
    pub const LEFTSHIFT: u16 = 42;
    pub const BACKSLASH: u16 = 43;
    pub const Z: u16 = 44;
    pub const X: u16 = 45;
    pub const C: u16 = 46;
    pub const V: u16 = 47;
    pub const B: u16 = 48;
    pub const N: u16 = 49;
    pub const M: u16 = 50;
    pub const COMMA: u16 = 51;
    pub const DOT: u16 = 52;
    pub const SLASH: u16 = 53;
    pub const RIGHTSHIFT: u16 = 54;
    pub const KPASTERISK: u16 = 55;
    pub const LEFTALT: u16 = 56;
    pub const SPACE: u16 = 57;
    pub const CAPSLOCK: u16 = 58;
    pub const F1: u16 = 59;
    pub const F2: u16 = 60;
    pub const F3: u16 = 61;
    pub const F4: u16 = 62;
    pub const F5: u16 = 63;
    pub const F6: u16 = 64;
    pub const F7: u16 = 65;
    pub const F8: u16 = 66;
    pub const F9: u16 = 67;
    pub const F10: u16 = 68;
    pub const NUMLOCK: u16 = 69;
    pub const SCROLLLOCK: u16 = 70;
    pub const KP7: u16 = 71;
    pub const KP8: u16 = 72;
    pub const KP9: u16 = 73;
    pub const KPMINUS: u16 = 74;
    pub const KP4: u16 = 75;
    pub const KP5: u16 = 76;
    pub const KP6: u16 = 77;
    pub const KPPLUS: u16 = 78;
    pub const KP1: u16 = 79;
    pub const KP2: u16 = 80;
    pub const KP3: u16 = 81;
    pub const KP0: u16 = 82;
    pub const KPDOT: u16 = 83;
    pub const ZENKAKUHANKAKU: u16 = 85;
    pub const KEY102ND: u16 = 86;
    pub const F11: u16 = 87;
    pub const F12: u16 = 88;
    pub const RO: u16 = 89;
    pub const KATAKANA: u16 = 90;
    pub const HIRAGANA: u16 = 91;
    pub const HENKAN: u16 = 92;
    pub const KATAKANAHIRAGANA: u16 = 93;
    pub const MUHENKAN: u16 = 94;
    pub const KPJPCOMMA: u16 = 95;
    pub const KPENTER: u16 = 96;
    pub const RIGHTCTRL: u16 = 97;
    pub const KPSLASH: u16 = 98;
    pub const SYSRQ: u16 = 99;
    pub const RIGHTALT: u16 = 100;
    pub const LINEFEED: u16 = 101;
    pub const HOME: u16 = 102;
    pub const UP: u16 = 103;
    pub const PAGEUP: u16 = 104;
    pub const LEFT: u16 = 105;
    pub const RIGHT: u16 = 106;
    pub const END: u16 = 107;
    pub const DOWN: u16 = 108;
    pub const PAGEDOWN: u16 = 109;
    pub const INSERT: u16 = 110;
    pub const DELETE: u16 = 111;
    pub const MACRO: u16 = 112;
    pub const MUTE: u16 = 113;
    pub const VOLUMEDOWN: u16 = 114;
    pub const VOLUMEUP: u16 = 115;
    pub const POWER: u16 = 116;
    pub const KPEQUAL: u16 = 117;
    pub const KPPLUSMINUS: u16 = 118;
    pub const PAUSE: u16 = 119;
    pub const SCALE: u16 = 120;
    pub const KPCOMMA: u16 = 121;
    pub const HANGEUL: u16 = 122;
    pub const HANJA: u16 = 123;
    pub const YEN: u16 = 124;
    pub const LEFTMETA: u16 = 125;
    pub const RIGHTMETA: u16 = 126;
    pub const COMPOSE: u16 = 127;
    pub const STOP: u16 = 128;
    pub const AGAIN: u16 = 129;
    pub const PROPS: u16 = 130;
    pub const UNDO: u16 = 131;
    pub const FRONT: u16 = 132;
    pub const COPY: u16 = 133;
    pub const OPEN: u16 = 134;
    pub const PASTE: u16 = 135;
    pub const FIND: u16 = 136;
    pub const CUT: u16 = 137;
    pub const HELP: u16 = 138;
    pub const MENU: u16 = 139;
    pub const CALC: u16 = 140;
    pub const SETUP: u16 = 141;
    pub const SLEEP: u16 = 142;
    pub const WAKEUP: u16 = 143;
    pub const FILE: u16 = 144;
    pub const SENDFILE: u16 = 145;
    pub const DELETEFILE: u16 = 146;
    pub const XFER: u16 = 147;
    pub const PROG1: u16 = 148;
    pub const PROG2: u16 = 149;
    pub const WWW: u16 = 150;
    pub const MSDOS: u16 = 151;
    pub const COFFEE: u16 = 152;
    pub const DIRECTION: u16 = 153;
    pub const CYCLEWINDOWS: u16 = 154;
    pub const MAIL: u16 = 155;
    pub const BOOKMARKS: u16 = 156;
    pub const COMPUTER: u16 = 157;
    pub const BACK: u16 = 158;
    pub const FORWARD: u16 = 159;
    pub const CLOSECD: u16 = 160;
    pub const EJECTCD: u16 = 161;
    pub const EJECTCLOSECD: u16 = 162;
    pub const NEXTSONG: u16 = 163;
    pub const PLAYPAUSE: u16 = 164;
    pub const PREVIOUSSONG: u16 = 165;
    pub const STOPCD: u16 = 166;
    pub const RECORD: u16 = 167;
    pub const REWIND: u16 = 168;
    pub const PHONE: u16 = 169;
    pub const ISO: u16 = 170;
    pub const CONFIG: u16 = 171;
    pub const HOMEPAGE: u16 = 172;
    pub const REFRESH: u16 = 173;
    pub const EXIT: u16 = 174;
    pub const MOVE: u16 = 175;
    pub const EDIT: u16 = 176;
    pub const SCROLLUP: u16 = 177;
    pub const SCROLLDOWN: u16 = 178;
    pub const KPLEFTPAREN: u16 = 179;
    pub const KPRIGHTPAREN: u16 = 180;
    pub const NEW: u16 = 181;
    pub const REDO: u16 = 182;
    pub const F13: u16 = 183;
    pub const F14: u16 = 184;
    pub const F15: u16 = 185;
    pub const F16: u16 = 186;
    pub const F17: u16 = 187;
    pub const F18: u16 = 188;
    pub const F19: u16 = 189;
    pub const F20: u16 = 190;
    pub const F21: u16 = 191;
    pub const F22: u16 = 192;
    pub const F23: u16 = 193;
    pub const F24: u16 = 194;
    pub const PLAYCD: u16 = 200;
    pub const PAUSECD: u16 = 201;
    pub const PROG3: u16 = 202;
    pub const PROG4: u16 = 203;
    pub const DASHBOARD: u16 = 204;
    pub const SUSPEND: u16 = 205;
    pub const CLOSE: u16 = 206;
    pub const PLAY: u16 = 207;
    pub const FASTFORWARD: u16 = 208;
    pub const BASSBOOST: u16 = 209;
    pub const PRINT: u16 = 210;
    pub const HP: u16 = 211;
    pub const CAMERA: u16 = 212;
    pub const SOUND: u16 = 213;
    pub const QUESTION: u16 = 214;
    pub const EMAIL: u16 = 215;
    pub const CHAT: u16 = 216;
    pub const SEARCH: u16 = 217;
    pub const CONNECT: u16 = 218;
    pub const FINANCE: u16 = 219;
    pub const SPORT: u16 = 220;
    pub const SHOP: u16 = 221;
    pub const ALTERASE: u16 = 222;
    pub const CANCEL: u16 = 223;
    pub const BRIGHTNESSDOWN: u16 = 224;
    pub const BRIGHTNESSUP: u16 = 225;
    pub const MEDIA: u16 = 226;
    pub const SWITCHVIDEOMODE: u16 = 227;
    pub const KBDILLUMTOGGLE: u16 = 228;
    pub const KBDILLUMDOWN: u16 = 229;
    pub const KBDILLUMUP: u16 = 230;
    pub const SEND: u16 = 231;
    pub const REPLY: u16 = 232;
    pub const FORWARDMAIL: u16 = 233;
    pub const SAVE: u16 = 234;
    pub const DOCUMENTS: u16 = 235;
    pub const BATTERY: u16 = 236;
    pub const BLUETOOTH: u16 = 237;
    pub const WLAN: u16 = 238;
    pub const UWB: u16 = 239;
    pub const UNKNOWN: u16 = 240;
    pub const VIDEO_NEXT: u16 = 241;
    pub const VIDEO_PREV: u16 = 242;
    pub const BRIGHTNESS_CYCLE: u16 = 243;
    pub const BRIGHTNESS_AUTO: u16 = 244;
    pub const DISPLAY_OFF: u16 = 245;
    pub const WWAN: u16 = 246;
    pub const RFKILL: u16 = 247;
    pub const MICMUTE: u16 = 248;

    // Buttons (`BTN_*`): a sub-range of the key namespace. Linux defines
    // these as `BTN_*` macros, so the names carry the prefix even though the
    // codes live inside the key range.
    pub const BTN_MISC: u16 = 0x100;
    pub const BTN_0: u16 = 0x100;
    pub const BTN_1: u16 = 0x101;
    pub const BTN_2: u16 = 0x102;
    pub const BTN_3: u16 = 0x103;
    pub const BTN_4: u16 = 0x104;
    pub const BTN_5: u16 = 0x105;
    pub const BTN_6: u16 = 0x106;
    pub const BTN_7: u16 = 0x107;
    pub const BTN_8: u16 = 0x108;
    pub const BTN_9: u16 = 0x109;

    pub const BTN_MOUSE: u16 = 0x110;
    pub const BTN_LEFT: u16 = 0x110;
    pub const BTN_RIGHT: u16 = 0x111;
    pub const BTN_MIDDLE: u16 = 0x112;
    pub const BTN_SIDE: u16 = 0x113;
    pub const BTN_EXTRA: u16 = 0x114;
    pub const BTN_FORWARD: u16 = 0x115;
    pub const BTN_BACK: u16 = 0x116;
    pub const BTN_TASK: u16 = 0x117;

    pub const BTN_JOYSTICK: u16 = 0x120;
    pub const BTN_TRIGGER: u16 = 0x120;
    pub const BTN_THUMB: u16 = 0x121;
    pub const BTN_THUMB2: u16 = 0x122;
    pub const BTN_TOP: u16 = 0x123;
    pub const BTN_TOP2: u16 = 0x124;
    pub const BTN_PINKIE: u16 = 0x125;
    pub const BTN_BASE: u16 = 0x126;
    pub const BTN_BASE2: u16 = 0x127;
    pub const BTN_BASE3: u16 = 0x128;
    pub const BTN_BASE4: u16 = 0x129;
    pub const BTN_BASE5: u16 = 0x12a;
    pub const BTN_BASE6: u16 = 0x12b;
    pub const BTN_DEAD: u16 = 0x12f;

    pub const BTN_GAMEPAD: u16 = 0x130;
    pub const BTN_SOUTH: u16 = 0x130;
    pub const BTN_EAST: u16 = 0x131;
    pub const BTN_C: u16 = 0x132;
    pub const BTN_NORTH: u16 = 0x133;
    pub const BTN_WEST: u16 = 0x134;
    pub const BTN_Z: u16 = 0x135;
    pub const BTN_TL: u16 = 0x136;
    pub const BTN_TR: u16 = 0x137;
    pub const BTN_TL2: u16 = 0x138;
    pub const BTN_TR2: u16 = 0x139;
    pub const BTN_SELECT: u16 = 0x13a;
    pub const BTN_START: u16 = 0x13b;
    pub const BTN_MODE: u16 = 0x13c;
    pub const BTN_THUMBL: u16 = 0x13d;
    pub const BTN_THUMBR: u16 = 0x13e;

    pub const BTN_DIGI: u16 = 0x140;
    pub const BTN_TOOL_PEN: u16 = 0x140;
    pub const BTN_TOOL_RUBBER: u16 = 0x141;
    pub const BTN_TOOL_BRUSH: u16 = 0x142;
    pub const BTN_TOOL_PENCIL: u16 = 0x143;
    pub const BTN_TOOL_AIRBRUSH: u16 = 0x144;
    pub const BTN_TOOL_FINGER: u16 = 0x145;
    pub const BTN_TOOL_MOUSE: u16 = 0x146;
    pub const BTN_TOOL_LENS: u16 = 0x147;
    pub const BTN_TOOL_QUINTTAP: u16 = 0x148;
    pub const BTN_STYLUS3: u16 = 0x149;
    pub const BTN_TOUCH: u16 = 0x14a;
    pub const BTN_STYLUS: u16 = 0x14b;
    pub const BTN_STYLUS2: u16 = 0x14c;
    pub const BTN_TOOL_DOUBLETAP: u16 = 0x14d;
    pub const BTN_TOOL_TRIPLETAP: u16 = 0x14e;
    pub const BTN_TOOL_QUADTAP: u16 = 0x14f;

    pub const BTN_WHEEL: u16 = 0x150;
    pub const BTN_GEAR_DOWN: u16 = 0x150;
    pub const BTN_GEAR_UP: u16 = 0x151;
}

/// Relative axis codes (`REL_*`).
pub mod rel {
    pub const X: u16 = 0x00;
    pub const Y: u16 = 0x01;
    pub const Z: u16 = 0x02;
    pub const RX: u16 = 0x03;
    pub const RY: u16 = 0x04;
    pub const RZ: u16 = 0x05;
    pub const HWHEEL: u16 = 0x06;
    pub const DIAL: u16 = 0x07;
    pub const WHEEL: u16 = 0x08;
    pub const MISC: u16 = 0x09;
    pub const RESERVED: u16 = 0x0a;
    pub const WHEEL_HI_RES: u16 = 0x0b;
    pub const HWHEEL_HI_RES: u16 = 0x0c;
}

/// Absolute axis codes (`ABS_*`).
pub mod abs {
    pub const X: u16 = 0x00;
    pub const Y: u16 = 0x01;
    pub const Z: u16 = 0x02;
    pub const RX: u16 = 0x03;
    pub const RY: u16 = 0x04;
    pub const RZ: u16 = 0x05;
    pub const THROTTLE: u16 = 0x06;
    pub const RUDDER: u16 = 0x07;
    pub const WHEEL: u16 = 0x08;
    pub const GAS: u16 = 0x09;
    pub const BRAKE: u16 = 0x0a;
    pub const HAT0X: u16 = 0x10;
    pub const HAT0Y: u16 = 0x11;
    pub const HAT1X: u16 = 0x12;
    pub const HAT1Y: u16 = 0x13;
    pub const HAT2X: u16 = 0x14;
    pub const HAT2Y: u16 = 0x15;
    pub const HAT3X: u16 = 0x16;
    pub const HAT3Y: u16 = 0x17;
    pub const PRESSURE: u16 = 0x18;
    pub const DISTANCE: u16 = 0x19;
    pub const TILT_X: u16 = 0x1a;
    pub const TILT_Y: u16 = 0x1b;
    pub const TOOL_WIDTH: u16 = 0x1c;
    pub const MT_SLOT: u16 = 0x2f;
    pub const MT_TOUCH_MAJOR: u16 = 0x30;
    pub const MT_TOUCH_MINOR: u16 = 0x31;
    pub const MT_WIDTH_MAJOR: u16 = 0x32;
    pub const MT_WIDTH_MINOR: u16 = 0x33;
    pub const MT_ORIENTATION: u16 = 0x34;
    pub const MT_POSITION_X: u16 = 0x35;
    pub const MT_POSITION_Y: u16 = 0x36;
    pub const MT_TOOL_TYPE: u16 = 0x37;
    pub const MT_BLOB_ID: u16 = 0x38;
    pub const MT_TRACKING_ID: u16 = 0x39;
    pub const MT_PRESSURE: u16 = 0x3a;
    pub const MT_DISTANCE: u16 = 0x3b;
    pub const MT_TOOL_X: u16 = 0x3c;
    pub const MT_TOOL_Y: u16 = 0x3d;
}

/// LED codes (`LED_*`).
pub mod led {
    pub const NUML: u16 = 0x00;
    pub const CAPSL: u16 = 0x01;
    pub const SCROLLL: u16 = 0x02;
    pub const COMPOSE: u16 = 0x03;
    pub const KANA: u16 = 0x04;
    pub const SLEEP: u16 = 0x05;
    pub const SUSPEND: u16 = 0x06;
    pub const MUTE: u16 = 0x07;
    pub const MISC: u16 = 0x08;
    pub const MAIL: u16 = 0x09;
    pub const CHARGING: u16 = 0x0a;
}

/// Miscellaneous codes (`MSC_*`).
pub mod msc {
    pub const SERIAL: u16 = 0x00;
    pub const PULSELED: u16 = 0x01;
    pub const GESTURE: u16 = 0x02;
    pub const RAW: u16 = 0x03;
    pub const SCAN: u16 = 0x04;
    pub const TIMESTAMP: u16 = 0x05;
}

/// Sound codes (`SND_*`).
pub mod snd {
    pub const CLICK: u16 = 0x00;
    pub const BELL: u16 = 0x01;
    pub const TONE: u16 = 0x02;
}

/// Switch codes (`SW_*`).
pub mod sw {
    pub const LID: u16 = 0x00;
    pub const TABLET_MODE: u16 = 0x01;
    pub const HEADPHONE_INSERT: u16 = 0x02;
    pub const RFKILL_ALL: u16 = 0x03;
    pub const MICROPHONE_INSERT: u16 = 0x04;
    pub const DOCK: u16 = 0x05;
    pub const LINEOUT_INSERT: u16 = 0x06;
    pub const JACK_PHYSICAL_INSERT: u16 = 0x07;
    pub const VIDEOOUT_INSERT: u16 = 0x08;
    pub const CAMERA_LENS_COVER: u16 = 0x09;
    pub const KEYPAD_SLIDE: u16 = 0x0a;
    pub const FRONT_PROXIMITY: u16 = 0x0b;
    pub const ROTATE_LOCK: u16 = 0x0c;
}

/// Bus types for [`InputId::bustype`] (`BUS_*`).
pub mod bus {
    pub const NONE: u16 = 0x00;
    pub const PCI: u16 = 0x01;
    pub const ISAPNP: u16 = 0x02;
    pub const USB: u16 = 0x03;
    pub const HIL: u16 = 0x04;
    pub const BLUETOOTH: u16 = 0x05;
    pub const VIRTUAL: u16 = 0x06;
    pub const ISA: u16 = 0x10;
    pub const I8042: u16 = 0x11;
    pub const XTKBD: u16 = 0x12;
    pub const RS232: u16 = 0x13;
    pub const GAMEPORT: u16 = 0x14;
    pub const PARPORT: u16 = 0x15;
    pub const AMIGA: u16 = 0x16;
    pub const ADB: u16 = 0x17;
    pub const I2C: u16 = 0x18;
    pub const HOST: u16 = 0x19;
}

/// `UINPUT_MAX_NAME_SIZE`.
pub const UINPUT_MAX_NAME_SIZE: usize = 80;

/// Modern setup structure for `UI_DEV_SETUP` (`struct uinput_setup`).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct UinputSetup {
    pub id: InputId,
    pub name: [u8; UINPUT_MAX_NAME_SIZE],
    pub ff_effects_max: u32,
}

impl UinputSetup {
    pub const SIZE: usize = size_of::<Self>();

    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < Self::SIZE {
            return None;
        }
        let id = InputId::from_bytes(&buf[0..InputId::SIZE])?;
        let mut name = [0u8; UINPUT_MAX_NAME_SIZE];
        name.copy_from_slice(&buf[InputId::SIZE..InputId::SIZE + UINPUT_MAX_NAME_SIZE]);
        let ff_offset = InputId::SIZE + UINPUT_MAX_NAME_SIZE;
        let ff_effects_max = u32::from_ne_bytes(buf[ff_offset..ff_offset + 4].try_into().ok()?);
        Some(Self {
            id,
            name,
            ff_effects_max,
        })
    }
}

/// Setup structure for `UI_ABS_SETUP` (`struct uinput_abs_setup`).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct UinputAbsSetup {
    pub code: u16,
    pub absinfo: InputAbsInfo,
}

impl UinputAbsSetup {
    pub const SIZE: usize = size_of::<Self>();

    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < 4 + InputAbsInfo::SIZE {
            return None;
        }
        let code = u16::from_ne_bytes(buf[0..2].try_into().ok()?);
        // In C `struct uinput_abs_setup`, 2 bytes of padding follow `code` (u16)
        let absinfo = InputAbsInfo::from_bytes(&buf[4..4 + InputAbsInfo::SIZE])?;
        Some(Self { code, absinfo })
    }
}

/// `EVIOCG*` / `EVIOCS*` and `UI_*` request numbers.
///
/// The `EVIOCGNAME`, `EVIOCGBIT`, ... requests encode the caller's buffer
/// length in the size field, so they are built with `_ioc` at call time and
/// decoded with [`super::ioctl::ioc_size`].
pub mod ioctl {
    use super::*;
    use crate::uapi::ioctl::{IOC_READ, IOC_WRITE, _io, _ioc, _ior, _iow};

    /// Event device request type.
    pub const EV_TYPE: u32 = b'E' as u32;
    /// uinput request type.
    pub const UI_TYPE: u32 = b'U' as u32;

    pub const EVIOCGVERSION: u32 = _ior::<i32>(EV_TYPE, 0x01);
    pub const EVIOCGID: u32 = _ior::<InputId>(EV_TYPE, 0x02);
    pub const EVIOCGREP: u32 = _ior::<[u32; 2]>(EV_TYPE, 0x03);
    pub const EVIOCSREP: u32 = _iow::<[u32; 2]>(EV_TYPE, 0x03);
    /// `EVIOCGNAME(len)`: device name, NUL-terminated.
    pub fn eviocgname(len: usize) -> u32 {
        _ioc(IOC_READ, EV_TYPE, 0x06, len)
    }
    /// `EVIOCGPHYS(len)`: physical path, NUL-terminated.
    pub fn eviocgphys(len: usize) -> u32 {
        _ioc(IOC_READ, EV_TYPE, 0x07, len)
    }
    /// `EVIOCGUNIQ(len)`: unique identifier, NUL-terminated.
    pub fn eviocguniq(len: usize) -> u32 {
        _ioc(IOC_READ, EV_TYPE, 0x08, len)
    }
    /// `EVIOCGPROP(len)`: input device properties bitmap.
    pub fn eviocgprop(len: usize) -> u32 {
        _ioc(IOC_READ, EV_TYPE, 0x09, len)
    }
    /// `EVIOCGKEY(len)`: current key/button state bitmap.
    pub fn eviocgkey(len: usize) -> u32 {
        _ioc(IOC_READ, EV_TYPE, 0x18, len)
    }
    /// `EVIOCGLED(len)`: current LED state bitmap.
    pub fn eviocgled(len: usize) -> u32 {
        _ioc(IOC_READ, EV_TYPE, 0x19, len)
    }
    /// `EVIOCGSND(len)`: current sound state bitmap.
    pub fn eviocgsnd(len: usize) -> u32 {
        _ioc(IOC_READ, EV_TYPE, 0x1a, len)
    }
    /// `EVIOCGSW(len)`: current switch state bitmap.
    pub fn eviocgsw(len: usize) -> u32 {
        _ioc(IOC_READ, EV_TYPE, 0x1b, len)
    }
    /// `EVIOCGBIT(ev, len)`: capability bitmap of `ev`, or the `EV_*` bitmap.
    pub fn eviocgbit(ev: u16, len: usize) -> u32 {
        _ioc(IOC_READ, EV_TYPE, 0x20 + ev as u32, len)
    }
    /// `EVIOCGABS(abs)`: get absolute axis parameters.
    pub fn eviocgabs(abs: u16) -> u32 {
        _ioc(IOC_READ, EV_TYPE, 0x40 + abs as u32, InputAbsInfo::SIZE)
    }
    /// `EVIOCSABS(abs)`: set absolute axis parameters.
    pub fn eviocsabs(abs: u16) -> u32 {
        _ioc(IOC_WRITE, EV_TYPE, 0xc0 + abs as u32, InputAbsInfo::SIZE)
    }
    pub const EVIOCGRAB: u32 = _iow::<i32>(EV_TYPE, 0x90);
    pub const EVIOCREVOKE: u32 = _iow::<i32>(EV_TYPE, 0x91);
    pub const EVIOCSCLOCKID: u32 = _iow::<i32>(EV_TYPE, 0xa0);

    /// uinput version reported to userspace (`UI_GET_VERSION`).
    pub const UINPUT_VERSION: i32 = 5;
    pub const UI_DEV_CREATE: u32 = _io(UI_TYPE, 101);
    pub const UI_DEV_DESTROY: u32 = _io(UI_TYPE, 102);
    pub const UI_DEV_SETUP: u32 = _iow::<UinputSetup>(UI_TYPE, 3);
    pub const UI_ABS_SETUP: u32 = _iow::<UinputAbsSetup>(UI_TYPE, 4);
    pub const UI_GET_VERSION: u32 = _ior::<i32>(UI_TYPE, 0x2d);
    pub const UI_SET_EVBIT: u32 = _iow::<i32>(UI_TYPE, 100);
    pub const UI_SET_KEYBIT: u32 = _iow::<i32>(UI_TYPE, 101);
    pub const UI_SET_RELBIT: u32 = _iow::<i32>(UI_TYPE, 102);
    pub const UI_SET_ABSBIT: u32 = _iow::<i32>(UI_TYPE, 103);
    pub const UI_SET_MSCBIT: u32 = _iow::<i32>(UI_TYPE, 104);
    pub const UI_SET_LEDBIT: u32 = _iow::<i32>(UI_TYPE, 105);
    pub const UI_SET_SNDBIT: u32 = _iow::<i32>(UI_TYPE, 106);
    pub const UI_SET_FFBIT: u32 = _iow::<i32>(UI_TYPE, 107);
    pub const UI_SET_PHYS: u32 = _iow::<i32>(UI_TYPE, 108);
    pub const UI_SET_SWBIT: u32 = _iow::<i32>(UI_TYPE, 109);
    pub const UI_SET_PROPBIT: u32 = _iow::<i32>(UI_TYPE, 110);
    /// `UI_GET_SYSNAME(len)`: returns device name.
    pub fn ui_get_sysname(len: usize) -> u32 {
        _ioc(IOC_READ, UI_TYPE, 44, len)
    }
}

