use crate::proc::process::Process;
use crate::syscall::{SyscallResult};
use crate::vm::vma::VmaManager;
use ostd::Error;
use ostd::arch::cpu::context::UserContext;

pub const POLLIN: i16 = 0x0001;
pub const POLLOUT: i16 = 0x0004;
pub const POLLERR: i16 = 0x0008;

#[repr(C)]
#[derive(Default, Copy, Clone)]
pub struct PollFd {
    pub fd: i32,
    pub events: i16,
    pub revents: i16,
}

/// `poll()` — SYS_poll = 7
pub fn syscall_poll(
    arg0: usize,
    arg1: usize,
    _arg2: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let fds_ptr = arg0;
    let nfds = arg1;

    if nfds == 0 {
        return SyscallResult::from_result(Ok(0));
    }
    if fds_ptr == 0 {
        return SyscallResult::from_err(Error::InvalidArgs);
    }

    let proc = Process::current();
    let fd_table = proc.fd_table.lock();
    let mut ready_count = 0i32;

    for i in 0..nfds {
        let entry_ptr = fds_ptr + i * core::mem::size_of::<PollFd>();
        let mut fd_bytes = [0u8; 4];
        let mut events_bytes = [0u8; 2];

        if vm.copy_from_user(entry_ptr, &mut fd_bytes).is_err() {
            return SyscallResult::from_err(Error::InvalidArgs);
        }
        if vm.copy_from_user(entry_ptr + 4, &mut events_bytes).is_err() {
            return SyscallResult::from_err(Error::InvalidArgs);
        }

        let fd = i32::from_ne_bytes(fd_bytes);
        let events = i16::from_ne_bytes(events_bytes);

        let mut revents = 0i16;
        if fd >= 0 {
            if fd_table.get_fd(fd).is_ok() {
                revents = events & (POLLIN | POLLOUT);
                if revents == 0 {
                    revents = POLLIN | POLLOUT;
                }
                ready_count += 1;
            } else {
                revents = POLLERR;
                ready_count += 1;
            }
        }

        if vm
            .copy_to_user(entry_ptr + 6, &revents.to_ne_bytes())
            .is_err()
        {
            return SyscallResult::from_err(Error::InvalidArgs);
        }
    }

    SyscallResult::from_result(Ok(ready_count))
}

/// `ppoll()` — SYS_ppoll = 271
pub fn syscall_ppoll(
    arg0: usize,
    arg1: usize,
    arg2: usize,
    arg3: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    ctx: &mut UserContext,
) -> SyscallResult {
    syscall_poll(arg0, arg1, arg2, arg3, 0, 0, vm, ctx)
}

/// `select()` — SYS_select = 23
pub fn syscall_select(
    arg0: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let nfds = arg0 as i32;
    SyscallResult::from_result(Ok(nfds.max(0)))
}

/// `pselect6()` — SYS_pselect6 = 270
pub fn syscall_pselect6(
    arg0: usize,
    arg1: usize,
    arg2: usize,
    arg3: usize,
    arg4: usize,
    arg5: usize,
    vm: &VmaManager,
    ctx: &mut UserContext,
) -> SyscallResult {
    syscall_select(arg0, arg1, arg2, arg3, arg4, arg5, vm, ctx)
}
