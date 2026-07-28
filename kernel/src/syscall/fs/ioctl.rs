use crate::proc::process::Process;
use crate::syscall::{SyscallResult, to_continue_i32};
use crate::vm::vma::VmaManager;
use ostd::Error;
use ostd::arch::cpu::context::UserContext;

pub const TCGETS: usize = 0x5401;
pub const TCSETS: usize = 0x5402;
pub const TIOCGWINSZ: usize = 0x5413;
pub const FIONBIO: usize = 0x5421;
pub const FIONREAD: usize = 0x541B;

#[repr(C)]
#[derive(Default, Copy, Clone)]
pub struct Winsize {
    pub ws_row: u16,
    pub ws_col: u16,
    pub ws_xpixel: u16,
    pub ws_ypixel: u16,
}

/// `ioctl()` — SYS_ioctl = 16
pub fn syscall_ioctl(
    arg0: usize,
    arg1: usize,
    arg2: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let fd = arg0 as i32;
    let cmd = arg1;
    let proc = Process::current();
    let fd_table = proc.fd_table.lock();

    if fd_table.get_fd(fd).is_err() {
        return to_continue_i32(Err(Error::InvalidArgs));
    }

    match cmd {
        TIOCGWINSZ => {
            if arg2 == 0 {
                return to_continue_i32(Err(Error::InvalidArgs));
            }
            let ws = Winsize {
                ws_row: 24,
                ws_col: 80,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            let mut buf = [0u8; 8];
            buf[0..2].copy_from_slice(&ws.ws_row.to_ne_bytes());
            buf[2..4].copy_from_slice(&ws.ws_col.to_ne_bytes());
            buf[4..6].copy_from_slice(&ws.ws_xpixel.to_ne_bytes());
            buf[6..8].copy_from_slice(&ws.ws_ypixel.to_ne_bytes());
            to_continue_i32(vm.copy_to_user(arg2, &buf).map(|_| 0))
        }
        FIONBIO => {
            if arg2 == 0 {
                return to_continue_i32(Err(Error::InvalidArgs));
            }
            let mut val_bytes = [0u8; 4];
            if vm.copy_from_user(arg2, &mut val_bytes).is_err() {
                return to_continue_i32(Err(Error::InvalidArgs));
            }
            to_continue_i32(Ok(0))
        }
        TCGETS | TCSETS => to_continue_i32(Ok(0)),
        _ => to_continue_i32(Ok(0)),
    }
}
