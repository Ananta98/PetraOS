pub mod io;
pub mod proc;
pub mod signals;

use crate::arch::syscall::syscall::SyscallFrame;

/// Standard x86_64 System Call Numbers
pub const SYS_READ: u64 = 0;
pub const SYS_WRITE: u64 = 1;
pub const SYS_RT_SIGACTION: u64 = 13;
pub const SYS_RT_SIGPROCMASK: u64 = 14;
pub const SYS_RT_SIGRETURN: u64 = 15;
pub const SYS_YIELD: u64 = 24;
pub const SYS_EXIT: u64 = 60;
pub const SYS_KILL: u64 = 62;

/// POSIX Linux Error Numbers for System Calls
#[repr(i64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyscallError {
    EPERM = 1,
    ENOENT = 2,
    ESRCH = 3,
    EINTR = 4,
    EIO = 5,
    EBADF = 9,
    ECHILD = 10,
    EAGAIN = 11,
    ENOMEM = 12,
    EFAULT = 14,
    EINVAL = 22,
    ENOSYS = 38,
}

/// System Call Result Type (Idiomatic Rust Error Propagation)
pub type SyscallResult = Result<usize, SyscallError>;

pub trait SyscallReturnRaw {
    fn into_raw(self) -> u64;
}

impl SyscallReturnRaw for SyscallResult {
    fn into_raw(self) -> u64 {
        match self {
            Ok(val) => val as u64,
            Err(err) => (-(err as i64)) as u64,
        }
    }
}

/// Function pointer type for system call handlers
pub type SyscallHandler = fn(&mut SyscallFrame) -> SyscallResult;

/// Entry in the Asterinas-style System Call Table
#[derive(Copy, Clone)]
pub struct SyscallEntry {
    pub num: u64,
    pub name: &'static str,
    pub handler: SyscallHandler,
}

macro_rules! register_syscalls {
    ($( $num:path => ($name:expr, $handler:expr) ),* $(,)?) => {
        pub static SYSCALL_TABLE: &[SyscallEntry] = &[
            $(
                SyscallEntry {
                    num: $num,
                    name: $name,
                    handler: $handler,
                },
            )*
        ];
    };
}

// Entries in SYSCALL_TABLE must be kept sorted by system call number for binary search.
register_syscalls! {
    SYS_READ => ("read", io::sys_read),
    SYS_WRITE => ("write", io::sys_write),
    SYS_RT_SIGACTION => ("rt_sigaction", signals::sys_rt_sigaction),
    SYS_RT_SIGPROCMASK => ("rt_sigprocmask", signals::sys_rt_sigprocmask),
    SYS_RT_SIGRETURN => ("rt_sigreturn", signals::sys_rt_sigreturn),
    SYS_YIELD => ("yield", proc::sys_yield),
    SYS_EXIT => ("exit", proc::sys_exit),
    SYS_KILL => ("kill", signals::sys_kill),
}

/// System Call Dispatcher utilizing Asterinas-style Binary Search
pub fn dispatch(frame: &mut SyscallFrame) -> u64 {
    let sys_num = frame.syscall_num();
    let result = match SYSCALL_TABLE.binary_search_by_key(&sys_num, |entry| entry.num) {
        Ok(idx) => {
            let entry = &SYSCALL_TABLE[idx];
            log::trace!("Dispatching sys_{} (nr={})", entry.name, entry.num);
            (entry.handler)(frame)
        }
        Err(_) => {
            log::warn!("Unhandled system call nr: {}", sys_num);
            Err(SyscallError::ENOSYS)
        }
    };

    result.into_raw()
}
