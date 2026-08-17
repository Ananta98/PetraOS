//! Input/Output Control (ioctl) Subsystem
use crate::drivers::gpu::framebuffer::{fb_console_get_dimensions, FRAMEBUFFER};
use crate::fs::vfs::types::VfsError;

pub const TCGETS: u64 = 0x5401;
pub const TCSETS: u64 = 0x5402;
pub const TCSETSW: u64 = 0x5403;
pub const TCSETSF: u64 = 0x5404;
pub const TCGETA: u64 = 0x5405;
pub const TCSETA: u64 = 0x5406;
pub const TCSETAW: u64 = 0x5407;
pub const TCSETAF: u64 = 0x5408;
pub const TIOCSCTTY: u64 = 0x540E;
pub const TIOCGPGRP: u64 = 0x540F;
pub const TIOCSPGRP: u64 = 0x5410;
pub const TIOCGWINSZ: u64 = 0x5413;
pub const TIOCSWINSZ: u64 = 0x5414;

pub const FBIOGET_VSCREENINFO: u64 = 0x4600;
pub const FBIOPUT_VSCREENINFO: u64 = 0x4601;
pub const FBIOGET_FSCREENINFO: u64 = 0x4602;

pub const FB_VISUAL_TRUECOLOR: u32 = 2;
pub const FB_TYPE_PACKED_PIXELS: u32 = 0;

/// x86_64 Linux termios structure for terminal control.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Termios {
    pub c_iflag: u32,
    pub c_oflag: u32,
    pub c_cflag: u32,
    pub c_lflag: u32,
    pub c_line: u8,
    pub c_cc: [u8; 19],
    pub c_ispeed: u32,
    pub c_ospeed: u32,
}

impl Termios {
    pub const fn default_console() -> Self {
        let mut c_cc = [0u8; 19];
        c_cc[0] = 3;   // VINTR (^C)
        c_cc[1] = 28;  // VQUIT (^\)
        c_cc[2] = 127; // VERASE (DEL/Backspace)
        c_cc[3] = 21;  // VKILL (^U)
        c_cc[4] = 4;   // VEOF (^D)
        c_cc[5] = 0;   // VTIME
        c_cc[6] = 1;   // VMIN
        c_cc[7] = 0;   // VSWTC
        c_cc[8] = 17;  // VSTART (^Q)
        c_cc[9] = 19;  // VSTOP (^S)
        c_cc[10] = 26; // VSUSP (^Z)
        c_cc[11] = 0;  // VEOL
        c_cc[12] = 18; // VREPRINT (^R)
        c_cc[13] = 15; // VDISCARD (^O)
        c_cc[14] = 23; // VWERASE (^W)
        c_cc[15] = 22; // VLNEXT (^V)
        c_cc[16] = 0;  // VEOL2
        c_cc[17] = 0;
        c_cc[18] = 0;

        Self {
            c_iflag: 0x0500, // ICRNL | IXON
            c_oflag: 0x0005, // OPOST | ONLCR
            c_cflag: 0x00bf, // B38400 | CS8 | CREAD | HUPCL
            c_lflag: 0x8a3b, // ISIG | ICANON | ECHO | ECHOE | ECHOK | ECHOCTL | ECHOKE
            c_line: 0,
            c_cc,
            c_ispeed: 38400,
            c_ospeed: 38400,
        }
    }
}

pub static CONSOLE_TERMIOS: crate::sync::spinlock::Spinlock<Termios> =
    crate::sync::spinlock::Spinlock::new(Termios::default_console());

/// x86_64 Linux winsize structure for window size control.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct WinSize {
    pub ws_row: u16,
    pub ws_col: u16,
    pub ws_xpixel: u16,
    pub ws_ypixel: u16,
}

/// Description of a bitfield inside a pixel for Framebuffer.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct FbBitfield {
    pub offset: u32,
    pub length: u32,
    pub msb_right: u32,
}

/// Variable screen info structure for FBIOGET_VSCREENINFO/FBIOPUT_VSCREENINFO.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct FbVarScreeninfo {
    pub xres: u32,
    pub yres: u32,
    pub xres_virtual: u32,
    pub yres_virtual: u32,
    pub xoffset: u32,
    pub yoffset: u32,
    pub bits_per_pixel: u32,
    pub grayscale: u32,
    pub red: FbBitfield,
    pub green: FbBitfield,
    pub blue: FbBitfield,
    pub transp: FbBitfield,
    pub nonstd: u32,
    pub activate: u32,
    pub height: u32,
    pub width: u32,
    pub accel_flags: u32,
    pub pixclock: u32,
    pub left_margin: u32,
    pub right_margin: u32,
    pub upper_margin: u32,
    pub lower_margin: u32,
    pub hsync_len: u32,
    pub vsync_len: u32,
    pub sync: u32,
    pub vmode: u32,
    pub rotate: u32,
    pub colorspace: u32,
    pub reserved: [u32; 4],
}

/// Fixed screen info structure for FBIOGET_FSCREENINFO.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FbFixScreeninfo {
    pub id: [u8; 16],
    pub smem_start: u64,
    pub smem_len: u32,
    pub type_: u32,
    pub type_aux: u32,
    pub visual: u32,
    pub xpanstep: u16,
    pub ypanstep: u16,
    pub ywrapstep: u16,
    pub line_length: u32,
    pub mmio_start: u64,
    pub mmio_len: u32,
    pub accel: u32,
    pub capabilities: u16,
    pub reserved: [u16; 2],
}

/// Check if a file descriptor corresponds to a TTY device.
pub fn isatty(fd: i32) -> Result<bool, VfsError> {
    if fd < 0 {
        return Err(VfsError::BadFd);
    }

    let proc_arc = crate::proc::current_process().ok_or(VfsError::NotFound)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(fd)?;
    drop(proc);

    let is_tty = fd == 0
        || fd == 1
        || fd == 2
        || file.dentry.inode.inode_type == crate::fs::vfs::types::InodeType::CharDevice;

    Ok(is_tty)
}

/// Dispatch ioctl commands on file descriptors.
pub fn do_ioctl(fd: i32, cmd: u64, arg: usize) -> Result<usize, VfsError> {
    if fd < 0 {
        return Err(VfsError::BadFd);
    }

    let proc_arc = crate::proc::current_process().ok_or(VfsError::NotFound)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(fd)?;
    drop(proc);

    let is_char_dev = fd == 0
        || fd == 1
        || fd == 2
        || file.dentry.inode.inode_type == crate::fs::vfs::types::InodeType::CharDevice;

    let arg_ptr = arg as *mut u8;

    match cmd {
        TCGETS => {
            if !is_char_dev {
                return Err(VfsError::NotSupported);
            }
            if !arg_ptr.is_null() {
                if !crate::syscalls::is_user_ptr_valid(arg_ptr as u64, core::mem::size_of::<Termios>()) {
                    return Err(VfsError::InvalidInput);
                }
                let term = *CONSOLE_TERMIOS.lock();
                // SAFETY: arg pointer validated with is_user_ptr_valid.
                unsafe {
                    core::ptr::write_volatile(arg_ptr as *mut Termios, term);
                }
            }
            Ok(0)
        }
        TIOCGWINSZ => {
            if !is_char_dev {
                return Err(VfsError::NotSupported);
            }
            if !arg_ptr.is_null() {
                if !crate::syscalls::is_user_ptr_valid(arg_ptr as u64, core::mem::size_of::<WinSize>()) {
                    return Err(VfsError::InvalidInput);
                }

                let (rows, cols) = fb_console_get_dimensions().unwrap_or((24, 80));
                let (xpixel, ypixel) = if let Some(ref fb) = *FRAMEBUFFER.lock() {
                    (fb.width() as u16, fb.height() as u16)
                } else {
                    (0, 0)
                };

                let ws = WinSize {
                    ws_row: rows as u16,
                    ws_col: cols as u16,
                    ws_xpixel: xpixel,
                    ws_ypixel: ypixel,
                };
                // SAFETY: arg pointer validated with is_user_ptr_valid.
                unsafe {
                    core::ptr::write_volatile(arg_ptr as *mut WinSize, ws);
                }
            }
            Ok(0)
        }
        TCSETS | TCSETSW | TCSETSF => {
            if !arg_ptr.is_null() {
                if !crate::syscalls::is_user_ptr_valid(arg_ptr as u64, core::mem::size_of::<Termios>()) {
                    return Err(VfsError::InvalidInput);
                }
                // SAFETY: arg pointer validated with is_user_ptr_valid.
                let new_term = unsafe { core::ptr::read_volatile(arg_ptr as *const Termios) };
                *CONSOLE_TERMIOS.lock() = new_term;
            }
            Ok(0)
        }
        TCGETA
        | TCSETA
        | TCSETAW
        | TCSETAF
        | TIOCSCTTY
        | TIOCSWINSZ
        | TIOCSPGRP => Ok(0),
        TIOCGPGRP => {
            if !arg_ptr.is_null() {
                if !crate::syscalls::is_user_ptr_valid(arg_ptr as u64, core::mem::size_of::<i32>()) {
                    return Err(VfsError::InvalidInput);
                }
                let pgid = proc_arc.lock().pgid.as_u64() as i32;
                // SAFETY: arg pointer validated with is_user_ptr_valid.
                unsafe {
                    core::ptr::write_volatile(arg_ptr as *mut i32, pgid);
                }
            }
            Ok(0)
        }
        FBIOGET_VSCREENINFO => {
            if arg_ptr.is_null() {
                return Err(VfsError::InvalidInput);
            }
            if !crate::syscalls::is_user_ptr_valid(arg_ptr as u64, core::mem::size_of::<FbVarScreeninfo>()) {
                return Err(VfsError::InvalidInput);
            }

            let fb_guard = FRAMEBUFFER.lock();
            let fb = fb_guard.as_ref().ok_or(VfsError::NotFound)?;
            let info = fb.info();

            let var_info = FbVarScreeninfo {
                xres: info.width as u32,
                yres: info.height as u32,
                xres_virtual: info.width as u32,
                yres_virtual: info.height as u32,
                xoffset: 0,
                yoffset: 0,
                bits_per_pixel: info.bpp as u32,
                grayscale: 0,
                red: FbBitfield {
                    offset: info.red_mask_shift as u32,
                    length: info.red_mask_size as u32,
                    msb_right: 0,
                },
                green: FbBitfield {
                    offset: info.green_mask_shift as u32,
                    length: info.green_mask_size as u32,
                    msb_right: 0,
                },
                blue: FbBitfield {
                    offset: info.blue_mask_shift as u32,
                    length: info.blue_mask_size as u32,
                    msb_right: 0,
                },
                transp: FbBitfield {
                    offset: 24,
                    length: 8,
                    msb_right: 0,
                },
                nonstd: 0,
                activate: 0,
                height: 0,
                width: 0,
                accel_flags: 0,
                pixclock: 0,
                left_margin: 0,
                right_margin: 0,
                upper_margin: 0,
                lower_margin: 0,
                hsync_len: 0,
                vsync_len: 0,
                sync: 0,
                vmode: 0,
                rotate: 0,
                colorspace: 0,
                reserved: [0; 4],
            };

            // SAFETY: arg pointer validated with is_user_ptr_valid.
            unsafe {
                core::ptr::write_volatile(arg_ptr as *mut FbVarScreeninfo, var_info);
            }
            Ok(0)
        }
        FBIOPUT_VSCREENINFO => {
            if arg_ptr.is_null() {
                return Err(VfsError::InvalidInput);
            }
            if !crate::syscalls::is_user_ptr_valid(arg_ptr as u64, core::mem::size_of::<FbVarScreeninfo>()) {
                return Err(VfsError::InvalidInput);
            }
            Ok(0)
        }
        FBIOGET_FSCREENINFO => {
            if arg_ptr.is_null() {
                return Err(VfsError::InvalidInput);
            }
            if !crate::syscalls::is_user_ptr_valid(arg_ptr as u64, core::mem::size_of::<FbFixScreeninfo>()) {
                return Err(VfsError::InvalidInput);
            }

            let fb_guard = FRAMEBUFFER.lock();
            let fb = fb_guard.as_ref().ok_or(VfsError::NotFound)?;
            let info = fb.info();

            let mut id = [0u8; 16];
            let name_bytes = b"petraos-fb";
            id[..name_bytes.len()].copy_from_slice(name_bytes);

            let fix_info = FbFixScreeninfo {
                id,
                smem_start: info.addr as u64,
                smem_len: fb.len() as u32,
                type_: FB_TYPE_PACKED_PIXELS,
                type_aux: 0,
                visual: FB_VISUAL_TRUECOLOR,
                xpanstep: 0,
                ypanstep: 0,
                ywrapstep: 0,
                line_length: info.pitch as u32,
                mmio_start: 0,
                mmio_len: 0,
                accel: 0,
                capabilities: 0,
                reserved: [0; 2],
            };

            // SAFETY: arg pointer validated with is_user_ptr_valid.
            unsafe {
                core::ptr::write_volatile(arg_ptr as *mut FbFixScreeninfo, fix_info);
            }
            Ok(0)
        }
        _ => Ok(0),
    }
}
