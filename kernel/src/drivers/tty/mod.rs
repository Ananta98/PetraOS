//! TTY & Terminal Subsystem
//!
//! Provides the primary terminal and console infrastructure, bridging hardware
//! input/display devices with POSIX termios line discipline and UNIX pseudo-terminals.
//!
//! Registered as a `device_initcall` so it initialises after hardware drivers
//! (keyboard, framebuffer) have been probed.

pub mod console;
pub mod pty;
pub mod termios;

pub use console::*;
pub use pty::*;
pub use termios::*;

use crate::fs::vfs::types::VfsError;
use core::sync::atomic::{AtomicBool, Ordering};

static TTY_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Initialize the entire TTY subsystem, including flanterm framebuffer console.
///
/// Idempotent — safe to call multiple times; only the first call has effect.
pub fn init() {
    if TTY_INITIALIZED.swap(true, Ordering::SeqCst) {
        return;
    }
    console::init();
    log::info!("[TTY] Subsystem initialized.");
}

/// Initcall wrapper so the TTY subsystem participates in the device initcall sequence.
pub fn tty_subsystem_init() -> Result<(), &'static str> {
    init();
    Ok(())
}

/// Read from the global console input buffer through line discipline.
/// If `non_blocking` is true, returns `Err(VfsError::WouldBlock)` if no input is ready.
pub fn tty_read(buf: &mut [u8], non_blocking: bool) -> Result<usize, VfsError> {
    if buf.is_empty() {
        return Ok(0);
    }

    loop {
        let mut guard = CONSOLE.lock();
        if let Some(ref mut c) = *guard {
            c.poll_input();
            let bytes_read = c.ldisc.read_bytes(buf);
            if bytes_read > 0 {
                return Ok(bytes_read);
            }
            if non_blocking {
                return Err(VfsError::WouldBlock);
            }
        } else {
            return Err(VfsError::NotFound);
        }
        drop(guard);

        // Check if there are pending signals interrupting the read.
        if let Some(proc_arc) = crate::proc::current_process() {
            let proc = proc_arc.lock();
            if proc.pending_signals.mask != 0 {
                return Err(VfsError::Interrupted);
            }
        }

        // Atomically enable interrupts and halt CPU until the next hardware interrupt.
        crate::arch::enable_and_hlt();
    }
}

/// Write bytes to the global console (renders to flanterm and serial mirror).
pub fn tty_write(buf: &[u8]) -> Result<usize, VfsError> {
    if let Some(ref mut c) = *CONSOLE.lock() {
        Ok(c.write_output(buf))
    } else {
        Err(VfsError::NotFound)
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

crate::device_initcall!(tty_subsystem_init);
crate::MODULE_LICENSE!("BSD-2-Clause");
crate::MODULE_AUTHOR!("Ananta98");
crate::MODULE_DESCRIPTION!("TTY & Terminal Subsystem");
crate::MODULE_VERSION!("1.0.0");
