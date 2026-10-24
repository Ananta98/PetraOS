// SPDX-License-Identifier: GPL-2.0

use core::mem::size_of;

// ----------------------------------------------------------------------------
// `ioctl_request` encoding (`uapi/asm-generic/ioctls.h` `_IOC()` macros)
// ----------------------------------------------------------------------------

/// Direction/type/number/size field widths of the standard encoding.
const IOC_NRBITS: u32 = 8;
const IOC_TYPEBITS: u32 = 8;
const IOC_SIZEBITS: u32 = 14;
const IOC_NRSHIFT: u32 = 0;
const IOC_TYPESHIFT: u32 = IOC_NRSHIFT + IOC_NRBITS;
const IOC_SIZESHIFT: u32 = IOC_TYPESHIFT + IOC_TYPEBITS;
const IOC_DIRSHIFT: u32 = IOC_SIZESHIFT + IOC_SIZEBITS;
/// `_IOC_READ`: the kernel writes the user-space buffer.
pub const IOC_READ: u32 = 1;
/// `_IOC_WRITE`: the kernel reads the user-space buffer.
pub const IOC_WRITE: u32 = 2;

/// `_IOC(dir, type, nr, size)`.
pub const fn _ioc(dir: u32, ty: u32, nr: u32, size: usize) -> u32 {
    ((dir & 0x3) << IOC_DIRSHIFT)
        | (((size as u32) & 0x3FFF) << IOC_SIZESHIFT)
        | ((ty & 0xFF) << IOC_TYPESHIFT)
        | ((nr & 0xFF) << IOC_NRSHIFT)
}

/// `_IO(type, nr)` — no payload.
pub const fn _io(ty: u32, nr: u32) -> u32 {
    _ioc(0, ty, nr, 0)
}

/// `_IOR(type, nr, size)` — kernel → user.
pub const fn _ior<T>(ty: u32, nr: u32) -> u32 {
    _ioc(IOC_READ, ty, nr, size_of::<T>())
}

/// `_IOW(type, nr, size)` — user → kernel.
pub const fn _iow<T>(ty: u32, nr: u32) -> u32 {
    _ioc(IOC_WRITE, ty, nr, size_of::<T>())
}

/// `_IOWR(type, nr, size)` — both directions.
pub const fn _iowr<T>(ty: u32, nr: u32) -> u32 {
    _ioc(IOC_READ | IOC_WRITE, ty, nr, size_of::<T>())
}

/// `_IOC_DIR(cmd)`: the direction field of a request number.
pub const fn ioc_dir(cmd: u32) -> u32 {
    cmd >> IOC_DIRSHIFT & 0x3
}

/// `_IOC_TYPE(cmd)`: the request type byte.
pub const fn ioc_type(cmd: u32) -> u32 {
    cmd >> IOC_TYPESHIFT & 0xFF
}

/// `_IOC_NR(cmd)`: the request number.
pub const fn ioc_nr(cmd: u32) -> u32 {
    cmd >> IOC_NRSHIFT & 0xFF
}

/// `_IOC_SIZE(cmd)`: the payload length encoded by the caller.
///
/// Variable-length requests such as `EVIOCGNAME(len)` carry their buffer size
/// here rather than in the request's own definition.
pub const fn ioc_size(cmd: u32) -> usize {
    (cmd >> IOC_SIZESHIFT & 0x3FFF) as usize
}

pub const TCGETS: u32 = 0x5401;
pub const TCSETS: u32 = 0x5402;
pub const TCSETSW: u32 = 0x5403;
pub const TCSETSF: u32 = 0x5404;

pub const TCSBRK: u32 = 0x5409;
pub const TCXONC: u32 = 0x540A;
pub const TCFLSH: u32 = 0x540B;

pub const TIOCOUTQ: u32 = 0x5411;
pub const TIOCGWINSZ: u32 = 0x5413;
pub const TIOCSWINSZ: u32 = 0x5414;

pub const TIOCGPGRP: u32 = 0x540F;
pub const TIOCSPGRP: u32 = 0x5410;

pub const TIOCSCTTY: u32 = 0x540E;
pub const TIOCNOTTY: u32 = 0x5422;
pub const TCSBRKP: u32 = 0x5425;
pub const TIOCSBRK: u32 = 0x5427;
pub const TIOCCBRK: u32 = 0x5428;
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
