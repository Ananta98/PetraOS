pub mod fs;
pub mod mm;
pub mod net;
pub mod proc;
pub mod scheduler;
pub mod signal;
pub mod time;

use crate::vm::vma::VmaManager;
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
    0   => fs::syscall_read,                       // SYS_read
    1   => fs::syscall_write,                      // SYS_write
    2   => fs::syscall_open,                       // SYS_open
    3   => fs::syscall_close,                      // SYS_close
    4   => fs::syscall_stat,                       // SYS_stat
    5   => fs::syscall_fstat,                      // SYS_fstat
    6   => fs::syscall_lstat,                      // SYS_lstat
    7   => fs::syscall_poll,                       // SYS_poll
    8   => fs::syscall_lseek,                      // SYS_lseek
    9   => mm::syscall_mmap,                       // SYS_mmap
    10  => mm::syscall_mprotect,                   // SYS_mprotect
    11  => mm::syscall_munmap,                     // SYS_munmap
    12  => mm::syscall_brk,                        // SYS_brk
    13  => signal::syscall_rt_sigaction,           // SYS_rt_sigaction
    14  => signal::syscall_rt_sigprocmask,         // SYS_rt_sigprocmask
    15  => signal::syscall_rt_sigreturn,           // SYS_rt_sigreturn
    16  => fs::syscall_ioctl,                      // SYS_ioctl
    21  => fs::syscall_access,                     // SYS_access
    22  => fs::syscall_pipe2,                      // SYS_pipe
    23  => fs::syscall_select,                     // SYS_select
    24  => scheduler::syscall_sched_yield,         // SYS_sched_yield
    25  => mm::syscall_mremap,                     // SYS_mremap
    26  => mm::syscall_msync,                      // SYS_msync
    28  => mm::syscall_madvise,                    // SYS_madvise
    29  => mm::syscall_shmget,                     // SYS_shmget
    30  => mm::syscall_shmat,                      // SYS_shmat
    31  => mm::syscall_shmctl,                     // SYS_shmctl
    32  => fs::syscall_dup,                        // SYS_dup
    33  => fs::syscall_dup2,                       // SYS_dup2
    34  => signal::syscall_rt_sigpending,          // SYS_rt_sigpending
    35  => time::syscall_nanosleep,                // SYS_nanosleep
    39  => proc::syscall_getpid,                   // SYS_getpid
    41  => net::syscall_socket,                    // SYS_socket
    42  => net::syscall_connect,                   // SYS_connect
    43  => net::syscall_accept,                    // SYS_accept
    44  => net::syscall_sendto,                    // SYS_sendto
    45  => net::syscall_recvfrom,                  // SYS_recvfrom
    46  => net::syscall_sendmsg,                   // SYS_sendmsg
    47  => net::syscall_recvmsg,                   // SYS_recvmsg
    48  => net::syscall_shutdown,                  // SYS_shutdown
    49  => net::syscall_bind,                      // SYS_bind
    50  => net::syscall_listen,                    // SYS_listen
    51  => net::syscall_getsockname,               // SYS_getsockname
    52  => net::syscall_getpeername,               // SYS_getpeername
    53  => net::syscall_socketpair,                // SYS_socketpair
    54  => net::syscall_setsockopt,                // SYS_setsockopt
    55  => net::syscall_getsockopt,                // SYS_getsockopt
    56  => proc::syscall_clone,                    // SYS_clone
    57  => proc::syscall_fork,                     // SYS_fork
    59  => proc::syscall_execve,                   // SYS_execve
    60  => proc::syscall_exit,                     // SYS_exit
    61  => proc::syscall_wait4,                    // SYS_wait4
    62  => signal::syscall_kill,                   // SYS_kill
    63  => proc::syscall_uname,                    // SYS_uname
    67  => mm::syscall_shmdt,                      // SYS_shmdt
    72  => fs::syscall_fcntl,                      // SYS_fcntl
    79  => fs::syscall_getcwd,                     // SYS_getcwd
    80  => fs::syscall_chdir,                      // SYS_chdir
    82  => fs::syscall_rename,                     // SYS_rename
    83  => fs::syscall_mkdir,                      // SYS_mkdir
    84  => fs::syscall_rmdir,                      // SYS_rmdir
    87  => fs::syscall_unlink,                     // SYS_unlink
    89  => fs::syscall_readlink,                   // SYS_readlink
    90  => fs::syscall_chmod,                      // SYS_chmod
    92  => fs::syscall_chown,                      // SYS_chown
    95  => fs::syscall_umask,                      // SYS_umask
    96  => time::syscall_gettimeofday,             // SYS_gettimeofday
    97  => proc::syscall_getrlimit,                // SYS_getrlimit
    99  => proc::syscall_sysinfo,                  // SYS_sysinfo
    101 => proc::syscall_ptrace,                   // SYS_ptrace
    102 => proc::syscall_getuid,                   // SYS_getuid
    104 => proc::syscall_getgid,                   // SYS_getgid
    105 => proc::syscall_setuid,                   // SYS_setuid
    106 => proc::syscall_setgid,                   // SYS_setgid
    107 => proc::syscall_geteuid,                  // SYS_geteuid
    108 => proc::syscall_getegid,                  // SYS_getegid
    109 => proc::syscall_setpgid,                  // SYS_setpgid
    110 => proc::syscall_getppid,                  // SYS_getppid
    112 => proc::syscall_setsid,                   // SYS_setsid
    113 => proc::syscall_setreuid,                 // SYS_setreuid
    114 => proc::syscall_setregid,                 // SYS_setregid
    117 => proc::syscall_setresuid,                // SYS_setresuid
    118 => proc::syscall_getresuid,                // SYS_getresuid
    119 => proc::syscall_setresgid,                // SYS_setresgid
    120 => proc::syscall_getresgid,                // SYS_getresgid
    121 => proc::syscall_getpgid,                  // SYS_getpgid
    122 => proc::syscall_setfsuid,                 // SYS_setfsuid
    123 => proc::syscall_setfsgid,                 // SYS_setfsgid
    124 => proc::syscall_getsid,                   // SYS_getsid
    128 => signal::syscall_rt_sigtimedwait,        // SYS_rt_sigtimedwait
    129 => signal::syscall_rt_sigqueueinfo,        // SYS_rt_sigqueueinfo
    130 => signal::syscall_rt_sigsuspend,          // SYS_rt_sigsuspend
    137 => fs::syscall_statfs,                     // SYS_statfs
    138 => fs::syscall_fstatfs,                    // SYS_fstatfs
    142 => scheduler::syscall_sched_setparam,      // SYS_sched_setparam
    143 => scheduler::syscall_sched_getparam,      // SYS_sched_getparam
    144 => scheduler::syscall_sched_setscheduler,  // SYS_sched_setscheduler
    145 => scheduler::syscall_sched_getscheduler,  // SYS_sched_getscheduler
    146 => scheduler::syscall_sched_get_priority_max, // SYS_sched_get_priority_max
    147 => scheduler::syscall_sched_get_priority_min, // SYS_sched_get_priority_min
    148 => scheduler::syscall_sched_rr_get_interval,  // SYS_sched_rr_get_interval
    158 => proc::syscall_arch_prctl,               // SYS_arch_prctl
    165 => fs::syscall_mount,                      // SYS_mount
    186 => proc::syscall_gettid,                   // SYS_gettid
    201 => time::syscall_time,                     // SYS_time
    202 => proc::syscall_futex,                    // SYS_futex
    203 => scheduler::syscall_sched_setaffinity,  // SYS_sched_setaffinity
    204 => scheduler::syscall_sched_getaffinity,  // SYS_sched_getaffinity
    213 => fs::syscall_epoll_create,               // SYS_epoll_create
    217 => fs::syscall_getdents64,                 // SYS_getdents64
    218 => proc::syscall_set_tid_address,          // SYS_set_tid_address
    228 => time::syscall_clock_gettime,            // SYS_clock_gettime
    229 => time::syscall_clock_getres,             // SYS_clock_getres
    231 => proc::syscall_exit_group,               // SYS_exit_group
    232 => fs::syscall_epoll_wait,                 // SYS_epoll_wait
    233 => fs::syscall_epoll_ctl,                  // SYS_epoll_ctl
    234 => signal::syscall_tgkill,                 // SYS_tgkill
    247 => proc::syscall_waitid,                   // SYS_waitid
    254 => fs::syscall_inotify_add_watch,          // SYS_inotify_add_watch
    255 => fs::syscall_inotify_rm_watch,           // SYS_inotify_rm_watch
    257 => fs::syscall_openat,                     // SYS_openat
    258 => fs::syscall_mkdirat,                    // SYS_mkdirat
    262 => fs::syscall_newfstatat,                 // SYS_newfstatat
    263 => fs::syscall_unlinkat,                   // SYS_unlinkat
    264 => fs::syscall_renameat,                   // SYS_renameat
    267 => fs::syscall_readlinkat,                 // SYS_readlinkat
    269 => fs::syscall_faccessat,                  // SYS_faccessat
    270 => fs::syscall_pselect6,                   // SYS_pselect6
    271 => fs::syscall_ppoll,                      // SYS_ppoll
    273 => proc::syscall_set_robust_list,          // SYS_set_robust_list
    281 => fs::syscall_epoll_pwait,                // SYS_epoll_pwait
    282 => signal::syscall_signalfd4,              // SYS_signalfd4
    283 => time::syscall_timerfd_create,           // SYS_timerfd_create
    284 => fs::syscall_eventfd,                    // SYS_eventfd
    286 => time::syscall_timerfd_settime,          // SYS_timerfd_settime
    287 => time::syscall_timerfd_gettime,          // SYS_timerfd_gettime
    288 => net::syscall_accept4,                   // SYS_accept4
    290 => fs::syscall_eventfd2,                   // SYS_eventfd2
    291 => fs::syscall_epoll_create1,              // SYS_epoll_create1
    292 => fs::syscall_dup3,                       // SYS_dup3
    293 => fs::syscall_pipe2,                      // SYS_pipe2
    294 => fs::syscall_inotify_init1,              // SYS_inotify_init1
    297 => signal::syscall_rt_tgsigqueueinfo,     // SYS_rt_tgsigqueueinfo
    302 => proc::syscall_prlimit64,                // SYS_prlimit64
    314 => scheduler::syscall_sched_setattr,      // SYS_sched_setattr
    315 => scheduler::syscall_sched_getattr,      // SYS_sched_getattr
    318 => proc::syscall_getrandom,                // SYS_getrandom
    319 => mm::syscall_memfd_create,               // SYS_memfd_create
}
