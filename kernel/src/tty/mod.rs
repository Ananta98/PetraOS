//! TTY & Terminal Subsystem
//!
//! Provides the primary terminal and console infrastructure, bridging hardware
//! input/display devices with POSIX termios line discipline and UNIX pseudo-terminals.

pub mod console;
pub mod pty;
pub mod termios;

pub use console::*;
pub use pty::*;
pub use termios::*;

use crate::fs::vfs::types::VfsError;

/// Initialize the entire TTY subsystem, including flanterm framebuffer console.
pub fn init() {
    console::init();
    log::info!("[TTY] Subsystem initialized.");
}

/// Read from the global console input buffer through line discipline.
/// Blocks until input is available (POSIX terminal blocking semantics).
pub fn tty_read(buf: &mut [u8]) -> Result<usize, VfsError> {
    if buf.is_empty() {
        return Ok(0);
    }

    loop {
        {
            let mut guard = CONSOLE.lock();
            if let Some(ref mut c) = *guard {
                c.poll_input();
                let bytes_read = c.ldisc.read_bytes(buf);
                if bytes_read > 0 {
                    return Ok(bytes_read);
                }
            } else {
                return Err(VfsError::NotFound);
            }
        }

        // Check if there are pending signals interrupting the read
        if let Some(proc_arc) = crate::proc::current_process() {
            let proc = proc_arc.lock();
            if proc.pending_signals.mask != 0 {
                return Err(VfsError::Interrupted);
            }
        }

        // Wait for keyboard interrupt or next timer interrupt
        crate::proc::thread::Thread::yield_cpu();
    }
}

/// Write bytes to the global console (renders to flanterm and serial mirror).
pub fn tty_write(buf: &[u8]) {
    if let Some(ref mut c) = *CONSOLE.lock() {
        let _ = c.write_output(buf);
    }
}

/// Global fallback ioctl dispatcher for terminal operations.
pub fn do_ioctl(fd: i32, cmd: u64, arg: usize) -> Result<usize, VfsError> {
    if fd < 0 {
        return Err(VfsError::BadFd);
    }

    let proc_arc = crate::proc::current_process().ok_or(VfsError::NotFound)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(fd)?;
    drop(proc);

    file.ops.ioctl(cmd, arg)
}

/// Test whether a file descriptor refers to a terminal device.
pub fn isatty(fd: i32) -> Result<bool, VfsError> {
    if fd < 0 {
        return Err(VfsError::BadFd);
    }

    let proc_arc = crate::proc::current_process().ok_or(VfsError::NotFound)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(fd)?;
    drop(proc);

    Ok(file.ops.isatty())
}
