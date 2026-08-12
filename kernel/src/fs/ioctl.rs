//! Input/Output Control (ioctl) Subsystem
use crate::fs::vfs::types::VfsError;

pub const TCGETS: u64 = 0x5401;
pub const TCSETS: u64 = 0x5402;
pub const TIOCGWINSZ: u64 = 0x5413;
pub const TIOCSWINSZ: u64 = 0x5414;
pub const TIOCGPGRP: u64 = 0x540F;
pub const TIOCSPGRP: u64 = 0x5410;

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

/// x86_64 Linux winsize structure for window size control.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct WinSize {
    pub ws_row: u16,
    pub ws_col: u16,
    pub ws_xpixel: u16,
    pub ws_ypixel: u16,
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

    let is_tty = fd == 0
        || fd == 1
        || fd == 2
        || file.dentry.inode.inode_type == crate::fs::vfs::types::InodeType::CharDevice;

    if !is_tty {
        return Err(VfsError::NotSupported);
    }

    let arg_ptr = arg as *mut u8;

    match cmd {
        TCGETS => {
            if !arg_ptr.is_null() {
                if !crate::syscalls::is_user_ptr_valid(arg_ptr as u64, core::mem::size_of::<Termios>()) {
                    return Err(VfsError::InvalidInput);
                }
                let term = Termios {
                    c_iflag: 0x0500, // ICRNL | IXON
                    c_oflag: 0x0005, // OPOST | ONLCR
                    c_cflag: 0x00bf, // B38400 | CS8 | CREAD | HUPCL
                    c_lflag: 0x8a3b, // ISIG | ICANON | ECHO | ECHOE | ECHOK | ECHOCTL | ECHOKE
                    c_line: 0,
                    c_cc: [0; 19],
                    c_ispeed: 38400,
                    c_ospeed: 38400,
                };
                // SAFETY: arg pointer validated with is_user_ptr_valid.
                unsafe {
                    core::ptr::write_volatile(arg_ptr as *mut Termios, term);
                }
            }
            Ok(0)
        }
        TIOCGWINSZ => {
            if !arg_ptr.is_null() {
                if !crate::syscalls::is_user_ptr_valid(arg_ptr as u64, core::mem::size_of::<WinSize>()) {
                    return Err(VfsError::InvalidInput);
                }
                let ws = WinSize {
                    ws_row: 24,
                    ws_col: 80,
                    ws_xpixel: 0,
                    ws_ypixel: 0,
                };
                // SAFETY: arg pointer validated with is_user_ptr_valid.
                unsafe {
                    core::ptr::write_volatile(arg_ptr as *mut WinSize, ws);
                }
            }
            Ok(0)
        }
        TCSETS | TIOCSWINSZ | TIOCSPGRP => Ok(0),
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
        _ => Ok(0),
    }
}
