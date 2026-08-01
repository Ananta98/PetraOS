use crate::proc::process::Process;
use crate::syscall::SyscallResult;
use crate::vm::vma::VmaManager;
use ostd::arch::cpu::context::UserContext;

/// `gettid()` — SYS_gettid = 186
pub fn syscall_gettid(
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    SyscallResult::from_result(Ok(Process::current().pid.as_u32() as i32))
}

/// `set_tid_address()` — SYS_set_tid_address = 218
pub fn syscall_set_tid_address(
    _tidptr: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    SyscallResult::from_result(Ok(Process::current().pid.as_u32() as i32))
}

/// `exit_group()` — SYS_exit_group = 231
pub fn syscall_exit_group(
    status: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    SyscallResult::Exit(status as i32)
}

/// `set_robust_list()` — SYS_set_robust_list = 273
pub fn syscall_set_robust_list(
    _head: usize,
    _len: usize,
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    SyscallResult::from_result(Ok(0))
}
