pub mod arch_prctl;
pub mod fs;
pub mod ipc;
pub mod ioctl;
pub mod mm;
pub mod proc;
pub mod signals;
pub mod sync;
pub mod sys_info;
pub mod time;

use crate::arch::syscall::SyscallFrame;
use crate::fs::vfs::types::*;
use crate::sync::futex::FutexError;

pub use crate::mm::user::{USER_SPACE_MAX_ADDR, UserCStr, UserPtr};

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
    EACCES = 13,
    EFAULT = 14,
    EEXIST = 17,
    ENODEV = 19,
    ENOTDIR = 20,
    EISDIR = 21,
    EINVAL = 22,
    EMFILE = 24,
    ENOTTY = 25,
    ESPIPE = 29,
    ENOSYS = 38,
    ELOOP = 40,
    EIDRM = 43,
    ERANGE = 34,
    ETIMEDOUT = 110,
}

impl From<FutexError> for SyscallError {
    fn from(err: FutexError) -> Self {
        match err {
            FutexError::WouldBlock => SyscallError::EAGAIN,
            FutexError::TimedOut => SyscallError::ETIMEDOUT,
            FutexError::InvalidArgument => SyscallError::EINVAL,
            FutexError::Fault => SyscallError::EFAULT,
            FutexError::Interrupted => SyscallError::EINTR,
            FutexError::NotSupported => SyscallError::ENOSYS,
        }
    }
}

impl From<VfsError> for SyscallError {
    fn from(err: VfsError) -> Self {
        match err {
            VfsError::NotFound => SyscallError::ENOENT,
            VfsError::NotDirectory => SyscallError::ENOTDIR,
            VfsError::NotFile => SyscallError::EINVAL,
            VfsError::AlreadyExists => SyscallError::EEXIST,
            VfsError::InvalidInput => SyscallError::EINVAL,
            VfsError::PermissionDenied => SyscallError::EPERM,
            VfsError::ReadOnlyFs => SyscallError::EPERM,
            VfsError::NotSupported => SyscallError::ENOSYS,
            VfsError::BadFd => SyscallError::EBADF,
            VfsError::NotEmpty => SyscallError::EINVAL,
            VfsError::IsDirectory => SyscallError::EISDIR,
            VfsError::Interrupted => SyscallError::EINTR,
            VfsError::TooManySymlinks => SyscallError::ELOOP,
            VfsError::DriverError(d) => match d {
                crate::device::DriverError::Timeout => SyscallError::ETIMEDOUT,
                crate::device::DriverError::NoDevice => SyscallError::ENODEV,
                crate::device::DriverError::AllocFailed => SyscallError::ENOMEM,
                crate::device::DriverError::Unsupported => SyscallError::ENOSYS,
                _ => SyscallError::EIO,
            },
        }
    }
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

/// Unified macro to define architecture-specific syscall numbers and construct the static dispatch table.
#[macro_export]
macro_rules! define_syscall_table {
    ($( $const_name:ident = $num:expr => ($name:expr, $handler:expr) ),* $(,)?) => {
        $(
            #[allow(dead_code)]
            pub const $const_name: u64 = $num;
        )*

        pub static SYSCALL_TABLE: &[$crate::syscalls::SyscallEntry] = &[
            $(
                $crate::syscalls::SyscallEntry {
                    num: $num,
                    name: $name,
                    handler: $handler,
                },
            )*
        ];
    };
}

/// System Call Dispatcher utilizing Asterinas-style Binary Search on architecture-specific table
pub fn dispatch(frame: &mut SyscallFrame) -> u64 {
    let sys_num = frame.syscall_num();
    let table = crate::arch::syscall::table::SYSCALL_TABLE;
    let result = match table.binary_search_by_key(&sys_num, |entry| entry.num) {
        Ok(idx) => {
            let entry = &table[idx];
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
