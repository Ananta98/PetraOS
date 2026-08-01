use crate::proc::process::Process;
use crate::syscall::{SyscallResult};
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
        return SyscallResult::from_err(Error::InvalidArgs);
    }

    match cmd {
        TIOCGWINSZ => {
            if arg2 == 0 {
                return SyscallResult::from_err(Error::InvalidArgs);
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
            SyscallResult::from_result(vm.copy_to_user(arg2, &buf).map(|_| 0))
        }
        FIONBIO => {
            if arg2 == 0 {
                return SyscallResult::from_err(Error::InvalidArgs);
            }
            let mut val_bytes = [0u8; 4];
            if vm.copy_from_user(arg2, &mut val_bytes).is_err() {
                return SyscallResult::from_err(Error::InvalidArgs);
            }
            SyscallResult::from_result(Ok(0))
        }
        TCGETS => {
            if arg2 != 0 {
                // Fill default Linux termios (36 bytes)
                // c_iflag=0x500 (ICRNL|IXON), c_oflag=0x5 (OPOST|ONLCR), c_cflag=0xbf (CS8|CREAD), c_lflag=0x8a3b (ISIG|ICANON|ECHO|ECHOE|ECHOK)
                let mut termios_bytes = [0u8; 36];
                let c_iflag: u32 = 0x500;
                let c_oflag: u32 = 0x5;
                let c_cflag: u32 = 0xbf;
                let c_lflag: u32 = 0x8a3b;
                termios_bytes[0..4].copy_from_slice(&c_iflag.to_ne_bytes());
                termios_bytes[4..8].copy_from_slice(&c_oflag.to_ne_bytes());
                termios_bytes[8..12].copy_from_slice(&c_cflag.to_ne_bytes());
                termios_bytes[12..16].copy_from_slice(&c_lflag.to_ne_bytes());
                termios_bytes[16] = 0; // c_line
                // Default control chars (c_cc)
                termios_bytes[17] = 0x03; // VINTR = Ctrl-C
                termios_bytes[18] = 0x1c; // VQUIT
                termios_bytes[19] = 0x7f; // ERASE
                termios_bytes[20] = 0x15; // VKILL
                termios_bytes[21] = 0x04; // EOF = Ctrl-D
                SyscallResult::from_result(vm.copy_to_user(arg2, &termios_bytes).map(|_| 0))
            } else {
                SyscallResult::from_result(Ok(0))
            }
        }
        TCSETS => SyscallResult::from_result(Ok(0)),
        _ => SyscallResult::from_result(Ok(0)),
    }
}
