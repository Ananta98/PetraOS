pub mod fs;
pub(crate) mod mm;
pub(crate) mod proc;
pub(crate) mod signal;

use crate::vm::vma::VmaManager;
use alloc::string::String;
use alloc::vec::Vec;
use ostd::Error;

/// The result of a system call dispatch.
pub enum SyscallResult {
    Continue(usize),
    Exit(i32),
}

// =============================================================================
// Marshalling helpers
//
// Shared by all `syscall_*` entry points (across the `fs` and `proc`
// submodules) to translate kernel results into the [`SyscallResult`] returned
// to user space and to copy data across the user/kernel boundary.
// =============================================================================

/// Converts a `Result<usize, Error>` into a [`SyscallResult::Continue`],
/// encoding the error code as a negated `isize` on failure.
pub(crate) fn to_continue(result: Result<usize, Error>) -> SyscallResult {
    match result {
        Ok(value) => SyscallResult::Continue(value),
        Err(error) => SyscallResult::Continue(-(error as isize) as usize),
    }
}

/// Adapts a `Result<i32, Error>` (file descriptor or signed return) into a
/// [`SyscallResult::Continue`], zero-extending the success value.
pub(crate) fn to_continue_i32(result: Result<i32, Error>) -> SyscallResult {
    to_continue(result.map(|value| value as usize))
}

/// Adapts a `Result<(), Error>` (no return value) into a
/// [`SyscallResult::Continue`] with a success value of `0`.
pub(crate) fn to_continue_unit(result: Result<(), Error>) -> SyscallResult {
    to_continue(result.map(|()| 0))
}

/// A unified handler signature for every registered system call.
///
/// Each handler is responsible for marshalling raw user arguments (and copying
/// data to/from user space via `vm`) and returning a [`SyscallResult`].
type SyscallHandler = fn(usize, usize, usize, usize, usize, usize, &VmaManager) -> SyscallResult;

/// Registers the system call dispatch table.
///
/// Each entry binds a system call number to its handler. The table must be kept
/// sorted by system call number so that `dispatch_syscall` can use binary
/// search. Adding a new system call is a single-line addition here.
macro_rules! syscall_table {
    ($($num:expr => $handler:expr),* $(,)?) => {
        const SYSCALL_TABLE: &[(usize, SyscallHandler)] = &[
            $(($num, $handler as SyscallHandler),)*
        ];
    };
}

syscall_table! {
    0   => fs::syscall_read,               // SYS_read
    1   => fs::syscall_write,              // SYS_write
    2   => fs::syscall_open,               // SYS_open
    3   => fs::syscall_close,              // SYS_close
    8   => fs::syscall_lseek,              // SYS_lseek
    9   => mm::syscall_mmap,               // SYS_mmap
    11  => mm::syscall_munmap,             // SYS_munmap
    12  => mm::syscall_brk,               // SYS_brk
    13  => signal::syscall_rt_sigaction,   // SYS_rt_sigaction
    14  => signal::syscall_rt_sigprocmask, // SYS_rt_sigprocmask
    15  => signal::syscall_rt_sigreturn,   // SYS_rt_sigreturn
    32  => fs::syscall_dup,               // SYS_dup
    33  => fs::syscall_dup2,              // SYS_dup2
    34  => signal::syscall_rt_sigpending,  // SYS_rt_sigpending
    60  => proc::syscall_exit,            // SYS_exit
    62  => signal::syscall_kill,          // SYS_kill
    72  => signal::syscall_rt_sigsuspend, // SYS_rt_sigsuspend
    165 => fs::syscall_mount,             // SYS_mount
    234 => signal::syscall_tgkill,        // SYS_tgkill
}

/// Dispatch system calls from user mode to their corresponding kernel implementations.
///
/// The dispatch uses a binary search over the compile-time [`SYSCALL_TABLE`],
/// which keeps the cost constant regardless of how many system calls are
/// registered. Unknown numbers fall back to `-EINVAL`.
pub fn dispatch_syscall(
    num: usize,
    arg0: usize,
    arg1: usize,
    arg2: usize,
    arg3: usize,
    arg4: usize,
    arg5: usize,
    vm: &VmaManager,
) -> SyscallResult {
    match SYSCALL_TABLE.binary_search_by_key(&num, |(number, _)| *number) {
        Ok(index) => SYSCALL_TABLE[index].1(arg0, arg1, arg2, arg3, arg4, arg5, vm),
        Err(_) => SyscallResult::Continue(-(Error::InvalidArgs as isize) as usize),
    }
}
