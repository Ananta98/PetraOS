use crate::proc::process::Process;
use crate::syscall::{SyscallResult};
use crate::vm::vma::VmaManager;
use ostd::Error;
use ostd::arch::cpu::context::UserContext;

/// `setsockopt()` — SYS_setsockopt = 54
pub fn syscall_setsockopt(
    arg0: usize,
    _level: usize,
    _optname: usize,
    _optval: usize,
    _optlen: usize,
    _: usize,
    _: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let fd = arg0 as i32;
    let proc = Process::current();
    let fd_table = proc.fd_table.lock();
    if fd_table.get_fd(fd).is_err() {
        return SyscallResult::from_err(Error::InvalidArgs);
    }
    SyscallResult::from_result(Ok(0))
}

/// `getsockopt()` — SYS_getsockopt = 55
pub fn syscall_getsockopt(
    arg0: usize,
    _level: usize,
    _optname: usize,
    optval: usize,
    optlen: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let fd = arg0 as i32;
    let proc = Process::current();
    let fd_table = proc.fd_table.lock();
    if fd_table.get_fd(fd).is_err() {
        return SyscallResult::from_err(Error::InvalidArgs);
    }

    if optval != 0 && optlen != 0 {
        let val: i32 = 0;
        let _ = vm.copy_to_user(optval, &val.to_ne_bytes());
        let len: u32 = 4;
        let _ = vm.copy_to_user(optlen, &len.to_ne_bytes());
    }
    SyscallResult::from_result(Ok(0))
}

/// `getsockname()` — SYS_getsockname = 51
pub fn syscall_getsockname(
    arg0: usize,
    addr_ptr: usize,
    len_ptr: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let fd = arg0 as i32;
    let proc = Process::current();
    let fd_table = proc.fd_table.lock();
    if fd_table.get_fd(fd).is_err() {
        return SyscallResult::from_err(Error::InvalidArgs);
    }

    if addr_ptr != 0 && len_ptr != 0 {
        // Return AF_INET (2) 127.0.0.1:8080
        let sockaddr: [u8; 16] = [
            2, 0, // AF_INET
            0x1F, 0x90, // port 8080
            127, 0, 0, 1, // 127.0.0.1
            0, 0, 0, 0, 0, 0, 0, 0,
        ];
        let _ = vm.copy_to_user(addr_ptr, &sockaddr);
        let len: u32 = 16;
        let _ = vm.copy_to_user(len_ptr, &len.to_ne_bytes());
    }
    SyscallResult::from_result(Ok(0))
}

/// `getpeername()` — SYS_getpeername = 52
pub fn syscall_getpeername(
    arg0: usize,
    addr_ptr: usize,
    len_ptr: usize,
    _: usize,
    _: usize,
    _: usize,
    vm: &VmaManager,
    ctx: &mut UserContext,
) -> SyscallResult {
    syscall_getsockname(arg0, addr_ptr, len_ptr, 0, 0, 0, vm, ctx)
}

/// `shutdown()` — SYS_shutdown = 48
pub fn syscall_shutdown(
    arg0: usize,
    _how: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let fd = arg0 as i32;
    let proc = Process::current();
    let fd_table = proc.fd_table.lock();
    if fd_table.get_fd(fd).is_err() {
        return SyscallResult::from_err(Error::InvalidArgs);
    }
    SyscallResult::from_result(Ok(0))
}
