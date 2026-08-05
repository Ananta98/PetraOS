use crate::vm::vma::VmaManager;
use ostd::arch::cpu::context::UserContext;
use ostd::Error;

/// Trait for types that can be converted into a system call return value register (`usize`).
pub trait IntoSyscallValue {
    fn into_syscall_value(self) -> usize;
}

impl IntoSyscallValue for usize {
    #[inline]
    fn into_syscall_value(self) -> usize {
        self
    }
}

impl IntoSyscallValue for i32 {
    #[inline]
    fn into_syscall_value(self) -> usize {
        self as usize
    }
}

impl IntoSyscallValue for () {
    #[inline]
    fn into_syscall_value(self) -> usize {
        0
    }
}

impl<T> IntoSyscallValue for *const T {
    #[inline]
    fn into_syscall_value(self) -> usize {
        self as usize
    }
}

impl<T> IntoSyscallValue for *mut T {
    #[inline]
    fn into_syscall_value(self) -> usize {
        self as usize
    }
}

/// The result of a system call dispatch.
pub enum SyscallResult {
    Return(usize),
    Exit(i32),
}

impl SyscallResult {
    /// Converts any kernel `Result<T, Error>` (where `T` implements [`IntoSyscallValue`])
    /// into a [`SyscallResult::Return`], encoding errors as a negated `isize` on failure.
    pub fn from_result<T: IntoSyscallValue>(result: Result<T, Error>) -> Self {
        match result {
            Ok(value) => SyscallResult::Return(value.into_syscall_value()),
            Err(error) => Self::from_err(error),
        }
    }

    /// Converts a kernel [`Error`] directly into a [`SyscallResult::Return`] with negated errno.
    pub fn from_err(error: Error) -> Self {
        let errno = match error {
            Error::InvalidArgs => 22,        // EINVAL (22)
            Error::AccessDenied => 13,       // EACCES (13)
            Error::NoMemory => 12,           // ENOMEM (12)
            Error::NotEnoughResources => 11, // EAGAIN (11)
            Error::IoError => 5,             // EIO (5)
            Error::PageFault => 14,          // EFAULT (14)
            Error::Overflow => 75,           // EOVERFLOW (75)
        };
        SyscallResult::Return(-(errno as isize) as usize)
    }
}

/// A unified handler signature for every registered system call.
///
/// Each handler is responsible for marshalling raw user arguments (and copying
/// data to/from user space via `vm`) and returning a [`SyscallResult`].
pub type SyscallHandler =
    fn(usize, usize, usize, usize, usize, usize, &VmaManager, &mut UserContext) -> SyscallResult;

/// Registers the system call dispatch table.
///
/// Each entry binds a system call number to its handler. The table must be kept
/// sorted by system call number so that `dispatch_syscall` can use binary
/// search. Adding a new system call is a single-line addition here.
macro_rules! syscall_table {
    ($($num:expr => $handler:expr),* $(,)?) => {
        pub const SYSCALL_TABLE: &[(usize, $crate::syscall::SyscallHandler)] = &[
            $(($num, $handler as $crate::syscall::SyscallHandler),)*
        ];
    };
}

pub mod arch;
pub mod fs;
pub mod mm;
pub mod net;
pub mod proc;
pub mod scheduler;
pub mod signal;
pub mod time;

use arch::SYSCALL_TABLE;

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
        Err(_) => SyscallResult::Return(-(Error::InvalidArgs as isize) as usize),
    }
}
