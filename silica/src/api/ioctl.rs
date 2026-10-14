// SPDX-License-Identifier: GPL-2.0

pub const TCGETS: u32 = 0x5401;
pub const TCSETS: u32 = 0x5402;
pub const TCSETSW: u32 = 0x5403;
pub const TCSETSF: u32 = 0x5404;

pub const TCSBRK: u32 = 0x5409;
pub const TCXONC: u32 = 0x540A;
pub const TCFLSH: u32 = 0x540B;

pub const TIOCGWINSZ: u32 = 0x5413;
pub const TIOCSWINSZ: u32 = 0x5414;

pub const TIOCGPGRP: u32 = 0x540F;
pub const TIOCSPGRP: u32 = 0x5410;

pub const TIOCSCTTY: u32 = 0x540E;
pub const TIOCNOTTY: u32 = 0x5422;
pub const FIONREAD: u32 = 0x541B;
pub const TIOCLINUX: u32 = 0x541C;

pub const TIOCGSID: u32 = 0x5429;

// ----------------------------------------------------------------------------
// Keyboard & Console Ioctls (Linux kd.h / kbd.h)
// ----------------------------------------------------------------------------

pub const KDGETLED: u32 = 0x4B31;
pub const KDSETLED: u32 = 0x4B32;
pub const KDGKBTYPE: u32 = 0x4B33;
pub const KDGETMODE: u32 = 0x4B3B;
pub const KDSETMODE: u32 = 0x4B3C;
pub const KDGKBMODE: u32 = 0x4B44;
pub const KDSKBMODE: u32 = 0x4B45;
pub const KDGKBENT: u32 = 0x4B46;
pub const KDSKBENT: u32 = 0x4B47;
pub const KDGKBDIACR: u32 = 0x4B4A;
pub const KDGETKEYCODE: u32 = 0x4B4C;
pub const KDSETKEYCODE: u32 = 0x4B4D;
pub const KDKBDREP: u32 = 0x4B52;
pub const GIO_FONT: u32 = 0x4B60;
pub const PIO_FONT: u32 = 0x4B61;
pub const KDGKBMETA: u32 = 0x4B62;
pub const KDSKBMETA: u32 = 0x4B63;
pub const KDGKBLED: u32 = 0x4B64;
pub const KDSKBLED: u32 = 0x4B65;
pub const GIO_CMAP: u32 = 0x4B70;
pub const PIO_CMAP: u32 = 0x4B71;

// Keyboard Mode constants
pub const K_RAW: u32 = 0x00;
pub const K_XLATE: u32 = 0x01;
pub const K_MEDIUMRAW: u32 = 0x02;
pub const K_UNICODE: u32 = 0x03;
pub const K_OFF: u32 = 0x04;

// Console Display Mode constants
pub const KD_TEXT: i32 = 0x00;
pub const KD_GRAPHICS: i32 = 0x01;

// Keyboard LED constants
pub const LED_SCR: u8 = 0x01;
pub const LED_NUM: u8 = 0x02;
pub const LED_CAP: u8 = 0x04;

// Meta Key Handling constants
pub const K_METABIT: u32 = 0x03;
pub const K_ESCPREFIX: u32 = 0x04;

// Keyboard Hardware Type
pub const KB_84: u8 = 0x01;
pub const KB_101: u8 = 0x02;

// ----------------------------------------------------------------------------
// Virtual Terminal (VT) Ioctls (Linux vt.h)
// ----------------------------------------------------------------------------

pub const VT_OPENQRY: u32 = 0x5600;
pub const VT_GETMODE: u32 = 0x5601;
pub const VT_SETMODE: u32 = 0x5602;
pub const VT_GETSTATE: u32 = 0x5603;
pub const VT_SENDSIG: u32 = 0x5604;
pub const VT_RELDISP: u32 = 0x5605;
pub const VT_ACTIVATE: u32 = 0x5606;
pub const VT_WAITACTIVE: u32 = 0x5607;
pub const VT_DISALLOCATE: u32 = 0x5608;
