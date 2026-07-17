pub mod fs;
pub mod mm;
pub mod proc;
pub mod signal;
pub mod time;

use crate::vm::vma::VmaManager;
use alloc::string::String;
use alloc::vec::Vec;
use ostd::Error;

use ostd::arch::cpu::context::UserContext;

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
pub fn to_continue(result: Result<usize, Error>) -> SyscallResult {
    match result {
        Ok(value) => SyscallResult::Continue(value),
        Err(error) => SyscallResult::Continue(-(error as isize) as usize),
    }
}

/// Adapts a `Result<i32, Error>` (file descriptor or signed return) into a
/// [`SyscallResult::Continue`], zero-extending the success value.
pub fn to_continue_i32(result: Result<i32, Error>) -> SyscallResult {
    to_continue(result.map(|value| value as usize))
}

/// Adapts a `Result<(), Error>` (no return value) into a
/// [`SyscallResult::Continue`] with a success value of `0`.
pub fn to_continue_unit(result: Result<(), Error>) -> SyscallResult {
    to_continue(result.map(|()| 0))
}

/// A unified handler signature for every registered system call.
///
/// Each handler is responsible for marshalling raw user arguments (and copying
/// data to/from user space via `vm`) and returning a [`SyscallResult`].
type SyscallHandler =
    fn(usize, usize, usize, usize, usize, usize, &VmaManager, &mut UserContext) -> SyscallResult;

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
    29  => mm::syscall_shmget,             // SYS_shmget
    30  => mm::syscall_shmat,              // SYS_shmat
    31  => mm::syscall_shmctl,             // SYS_shmctl
    32  => fs::syscall_dup,               // SYS_dup
    33  => fs::syscall_dup2,              // SYS_dup2
    34  => signal::syscall_rt_sigpending,  // SYS_rt_sigpending
    35  => time::syscall_nanosleep,        // SYS_nanosleep
    39  => proc::syscall_getpid,           // SYS_getpid
    57  => proc::syscall_fork,             // SYS_fork
    59  => proc::syscall_execve,           // SYS_execve
    60  => proc::syscall_exit,            // SYS_exit
    61  => proc::syscall_wait4,            // SYS_wait4
    62  => signal::syscall_kill,          // SYS_kill
    67  => mm::syscall_shmdt,              // SYS_shmdt
    72  => signal::syscall_rt_sigsuspend, // SYS_rt_sigsuspend
    96  => time::syscall_gettimeofday,     // SYS_gettimeofday
    102 => proc::syscall_getuid,           // SYS_getuid
    104 => proc::syscall_getgid,           // SYS_getgid
    105 => proc::syscall_setuid,           // SYS_setuid
    106 => proc::syscall_setgid,           // SYS_setgid
    107 => proc::syscall_geteuid,          // SYS_geteuid
    108 => proc::syscall_getegid,          // SYS_getegid
    109 => proc::syscall_setpgid,          // SYS_setpgid
    110 => proc::syscall_getppid,          // SYS_getppid
    112 => proc::syscall_setsid,           // SYS_setsid
    113 => proc::syscall_setreuid,         // SYS_setreuid
    114 => proc::syscall_setregid,         // SYS_setregid
    117 => proc::syscall_setresuid,        // SYS_setresuid
    118 => proc::syscall_getresuid,        // SYS_getresuid
    119 => proc::syscall_setresgid,        // SYS_setresgid
    120 => proc::syscall_getresgid,        // SYS_getresgid
    121 => proc::syscall_getpgid,          // SYS_getpgid
    122 => proc::syscall_setfsuid,         // SYS_setfsuid
    123 => proc::syscall_setfsgid,         // SYS_setfsgid
    124 => proc::syscall_getsid,           // SYS_getsid
    165 => fs::syscall_mount,             // SYS_mount
    201 => time::syscall_time,             // SYS_time
    228 => time::syscall_clock_gettime,    // SYS_clock_gettime
    229 => time::syscall_clock_getres,     // SYS_clock_getres
    234 => signal::syscall_tgkill,        // SYS_tgkill
    247 => proc::syscall_waitid,           // SYS_waitid
    293 => fs::syscall_pipe2,             // SYS_pipe2
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
    context: &mut UserContext,
) -> SyscallResult {
    match SYSCALL_TABLE.binary_search_by_key(&num, |(number, _)| *number) {
        Ok(index) => SYSCALL_TABLE[index].1(arg0, arg1, arg2, arg3, arg4, arg5, vm, context),
        Err(_) => SyscallResult::Continue(-(Error::InvalidArgs as isize) as usize),
    }
}

/// Helper to read a null-terminated string from user space.
pub fn read_user_string(vm: &VmaManager, user_ptr: usize) -> Result<String, Error> {
    let mut buf = Vec::new();
    let mut offset = 0;
    loop {
        let mut char_buf = [0u8; 1];
        vm.copy_from_user(user_ptr + offset, &mut char_buf)?;
        if char_buf[0] == 0 {
            break;
        }
        buf.push(char_buf[0]);
        offset += 1;
        if offset > 4096 {
            return Err(Error::InvalidArgs);
        }
    }
    String::from_utf8(buf).map_err(|_| Error::InvalidArgs)
}

/// Helper to read a byte slice from user space.
pub fn read_user_slice(
    vm: &VmaManager,
    user_ptr: usize,
    len: usize,
) -> Result<Vec<u8>, Error> {
    if len > 1024 * 1024 {
        return Err(Error::InvalidArgs);
    }
    let mut buf = alloc::vec![0u8; len];
    vm.copy_from_user(user_ptr, &mut buf)?;
    Ok(buf)
}
