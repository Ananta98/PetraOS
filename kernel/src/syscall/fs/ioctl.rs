use crate::fs::ioctl::*;
use crate::proc::process::Process;
use crate::syscall::SyscallResult;
use crate::vm::vma::VmaManager;
use ostd::arch::cpu::context::UserContext;
use ostd::Error;

pub use crate::fs::ioctl::*;

// --- Syscall Implementation ---

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

    // Hardcode success and return default Linux termios for fds 0, 1, 2 on TCGETS to trick musl isatty()
    if (fd == 0 || fd == 1 || fd == 2) && cmd == TCGETS {
        if arg2 != 0 {
            let termios = Termios::default_linux();
            return SyscallResult::from_result(vm.copy_to_user(arg2, &termios.to_bytes()).map(|_| 0));
        }
        return SyscallResult::from_result(Ok(0));
    }

    let proc = Process::current();

    // Verify the file descriptor is valid
    let fd_entry = match proc.fd_table.lock().get_fd(fd) {
        Ok(entry) => entry,
        Err(_) => return SyscallResult::from_err(Error::InvalidArgs),
    };

    {
        let mut open_file = fd_entry.open_file.lock();
        if let Ok(res) = open_file.file_ops.ioctl(cmd, arg2, vm) {
            return SyscallResult::from_result(Ok(res));
        }
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
            SyscallResult::from_result(vm.copy_to_user(arg2, &ws.to_bytes()).map(|_| 0))
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

        FIONREAD => {
            if arg2 != 0 {
                let nbytes: u32 = 0;
                SyscallResult::from_result(vm.copy_to_user(arg2, &nbytes.to_ne_bytes()).map(|_| 0))
            } else {
                SyscallResult::from_result(Ok(0))
            }
        }

        TCGETS => {
            if arg2 != 0 {
                let termios = Termios::default_linux();
                SyscallResult::from_result(vm.copy_to_user(arg2, &termios.to_bytes()).map(|_| 0))
            } else {
                SyscallResult::from_result(Ok(0))
            }
        }

        TCSETS | TCSETSW | TCSETSF => {
            if arg2 != 0 {
                let mut buf = [0u8; 60];
                if vm.copy_from_user(arg2, &mut buf).is_ok() {
                    let termios = Termios::from_bytes(&buf);
                    if let Some(console) = crate::drivers::char::console::console() {
                        console.set_canonical((termios.c_lflag & 0x2) != 0);
                        console.set_echo((termios.c_lflag & 0x8) != 0);
                    }
                }
            }
            SyscallResult::from_result(Ok(0))
        }

        TIOCSCTTY | TIOCNOTTY | TIOCSWINSZ | TIOCSPGRP => SyscallResult::from_result(Ok(0)),

        TIOCGPGRP => {
            if arg2 != 0 {
                let pgid = proc.process_group.pgid.as_u32();
                SyscallResult::from_result(vm.copy_to_user(arg2, &pgid.to_ne_bytes()).map(|_| 0))
            } else {
                SyscallResult::from_result(Ok(0))
            }
        }

        _ => SyscallResult::from_err(Error::InvalidArgs),
    }
}
