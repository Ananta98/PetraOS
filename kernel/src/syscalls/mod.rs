pub mod arch_prctl;
pub mod fs;
pub mod ioctl;
pub mod ipc;
pub mod mm;
pub mod net;
pub mod power;
pub mod proc;
pub mod random;
pub mod sched;
pub mod signals;
pub mod sync;
pub mod sys_info;
pub mod time;

pub use crate::arch::syscall::SyscallFrame;
pub use crate::device::DriverError;
pub use crate::fs::vfs::types::*;
pub use crate::mm::VirtAddr;
pub use crate::mm::user::{USER_SPACE_MAX_ADDR, UserCStr, UserPtr};
pub use crate::sync::futex::FutexError;
pub use crate::{define_syscall_table, sys_unimpl, syscall_adapter, wrap_syscall};
pub use paste;

/// POSIX Linux Error Numbers for System Calls
#[repr(i64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyscallError {
    EPERM = 1,
    ENOENT = 2,
    ESRCH = 3,
    EINTR = 4,
    EIO = 5,
    ENOEXEC = 8,
    EBADF = 9,
    ECHILD = 10,
    EAGAIN = 11,
    ENOMEM = 12,
    EACCES = 13,
    EFAULT = 14,
    EBUSY = 16,
    EEXIST = 17,
    ENODEV = 19,
    ENOTDIR = 20,
    EISDIR = 21,
    EINVAL = 22,
    EMFILE = 24,
    ENOTTY = 25,
    ENOSPC = 28,
    ESPIPE = 29,
    EROFS = 30,
    EPIPE = 32,
    ERANGE = 34,
    ENOSYS = 38,
    ENOTEMPTY = 39,
    ELOOP = 40,
    EIDRM = 43,
    ENOTSOCK = 88,
    EDESTADDRREQ = 89,
    EMSGSIZE = 90,
    EPROTOTYPE = 91,
    ENOPROTOOPT = 92,
    EPROTONOSUPPORT = 93,
    ESOCKTNOSUPPORT = 94,
    EOPNOTSUPP = 95,
    EPFNOSUPPORT = 96,
    EAFNOSUPPORT = 97,
    EADDRINUSE = 98,
    EADDRNOTAVAIL = 99,
    ENETDOWN = 100,
    ENETUNREACH = 101,
    ENETRESET = 102,
    ECONNABORTED = 103,
    ECONNRESET = 104,
    ENOBUFS = 105,
    EISCONN = 106,
    ENOTCONN = 107,
    ESHUTDOWN = 108,
    ETOOMANYREFS = 109,
    ETIMEDOUT = 110,
    ECONNREFUSED = 111,
    EHOSTDOWN = 112,
    EHOSTUNREACH = 113,
    EALREADY = 114,
    EINPROGRESS = 115,
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
            VfsError::ReadOnlyFs => SyscallError::EROFS,
            VfsError::NotSupported => SyscallError::ENOSYS,
            VfsError::BadFd => SyscallError::EBADF,
            VfsError::NotEmpty => SyscallError::ENOTEMPTY,
            VfsError::IsDirectory => SyscallError::EISDIR,
            VfsError::Interrupted => SyscallError::EINTR,
            VfsError::TooManySymlinks => SyscallError::ELOOP,
            VfsError::WouldBlock => SyscallError::EAGAIN,
            VfsError::NoSpace => SyscallError::ENOSPC,
            VfsError::DriverError(d) => match d {
                DriverError::Timeout => SyscallError::ETIMEDOUT,
                DriverError::NoDevice => SyscallError::ENODEV,
                DriverError::AllocFailed => SyscallError::ENOMEM,
                DriverError::Unsupported => SyscallError::ENOSYS,
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
    #[inline(always)]
    fn into_raw(self) -> u64 {
        match self {
            Ok(val) => val as u64,
            Err(err) => (-(err as i64)) as u64,
        }
    }
}

/// Trait for converting typed system call returns into `SyscallResult`.
pub trait SyscallReturn {
    fn into_syscall_result(self) -> SyscallResult;
}

impl SyscallReturn for SyscallResult {
    #[inline(always)]
    fn into_syscall_result(self) -> SyscallResult {
        self
    }
}

impl SyscallReturn for Result<(), SyscallError> {
    #[inline(always)]
    fn into_syscall_result(self) -> SyscallResult {
        self.map(|_| 0)
    }
}

macro_rules! impl_syscall_return_result {
    ($($typ:ty),*) => {
        $(
            impl SyscallReturn for Result<$typ, SyscallError> {
                #[inline(always)]
                fn into_syscall_result(self) -> SyscallResult {
                    self.map(|v| v as usize)
                }
            }
        )*
    };
}

impl_syscall_return_result!(u8, u16, u32, u64, i8, i16, i32, i64, isize);

impl SyscallReturn for Result<bool, SyscallError> {
    #[inline(always)]
    fn into_syscall_result(self) -> SyscallResult {
        self.map(|v| if v { 1 } else { 0 })
    }
}

impl SyscallReturn for Result<VirtAddr, SyscallError> {
    #[inline(always)]
    fn into_syscall_result(self) -> SyscallResult {
        self.map(|v| v.as_u64() as usize)
    }
}

impl SyscallReturn for () {
    #[inline(always)]
    fn into_syscall_result(self) -> SyscallResult {
        Ok(0)
    }
}

macro_rules! impl_syscall_return_primitive {
    ($($typ:ty),*) => {
        $(
            impl SyscallReturn for $typ {
                #[inline(always)]
                fn into_syscall_result(self) -> SyscallResult {
                    Ok(self as usize)
                }
            }
        )*
    };
}

impl_syscall_return_primitive!(u8, u16, u32, u64, usize, i8, i16, i32, i64, isize);

/// Trait for extracting strongly-typed arguments from raw 64-bit system call registers.
pub trait SyscallArg: Sized {
    fn from_arg(arg: u64) -> Result<Self, SyscallError>;

    #[inline(always)]
    fn from_usize(arg: usize) -> Result<Self, SyscallError> {
        Self::from_arg(arg as u64)
    }
}

macro_rules! impl_syscall_arg_int {
    ($($typ:ty),*) => {
        $(
            impl SyscallArg for $typ {
                #[inline(always)]
                fn from_arg(arg: u64) -> Result<Self, SyscallError> {
                    Ok(arg as $typ)
                }
            }
        )*
    };
}

impl_syscall_arg_int!(u8, u16, u32, u64, usize);
impl_syscall_arg_int!(i8, i16, i32, i64, isize);

impl SyscallArg for bool {
    #[inline(always)]
    fn from_arg(arg: u64) -> Result<Self, SyscallError> {
        Ok(arg != 0)
    }
}

impl SyscallArg for VirtAddr {
    #[inline(always)]
    fn from_arg(arg: u64) -> Result<Self, SyscallError> {
        Ok(VirtAddr::new(arg))
    }
}

impl<T: Sized + Copy> SyscallArg for UserPtr<T> {
    #[inline(always)]
    fn from_arg(arg: u64) -> Result<Self, SyscallError> {
        Ok(UserPtr::from_u64(arg))
    }
}

impl SyscallArg for UserCStr {
    #[inline(always)]
    fn from_arg(arg: u64) -> Result<Self, SyscallError> {
        Ok(UserCStr::from_u64(arg))
    }
}

impl<T> SyscallArg for *const T {
    #[inline(always)]
    fn from_arg(arg: u64) -> Result<Self, SyscallError> {
        Ok(arg as *const T)
    }
}

impl<T> SyscallArg for *mut T {
    #[inline(always)]
    fn from_arg(arg: u64) -> Result<Self, SyscallError> {
        Ok(arg as *mut T)
    }
}

impl<T: SyscallArg> SyscallArg for Option<T> {
    #[inline(always)]
    fn from_arg(arg: u64) -> Result<Self, SyscallError> {
        if arg == 0 {
            Ok(None)
        } else {
            T::from_arg(arg).map(Some)
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

/// Automatically wraps a strongly-typed system call handler function into a `SyscallHandler`
/// (`fn(&mut SyscallFrame) -> SyscallResult`).
///
/// Can be used as an attribute macro (`#[wrap_syscall]`) or as a function-like macro (`wrap_syscall! { ... }`).
///
/// # Example
/// ```rust
/// #[wrap_syscall]
/// pub fn writev(fd: i32, iov: VirtAddr, iovcnt: usize) -> SyscallResult {
///     // Implementation using typed arguments directly
///     Ok(0)
/// }
/// ```
#[macro_export]
macro_rules! wrap_syscall {
    // Special case for handlers directly accepting `&mut SyscallFrame`
    (@expand $(#[$meta:meta])* $vis:vis fn $name:ident ($frame:ident : &mut $($frame_ty:ident)::+ $(,)?) -> $ret:ty $body:block) => {
        $(#[$meta])*
        $vis fn $name($frame: &mut $crate::arch::syscall::SyscallFrame) -> $crate::syscalls::SyscallResult {
            fn inner($frame: &mut $crate::arch::syscall::SyscallFrame) -> $ret $body
            let res = inner($frame);
            $crate::syscalls::SyscallReturn::into_syscall_result(res)
        }
    };

    // Strongly-typed system call handler using paste to map arguments
    (@expand $(#[$meta:meta])* $vis:vis fn $name:ident (
        $( $($arg:ident)+ : $arg_ty:ty ),* $(,)?
    ) -> $ret:ty $body:block) => {
        $(#[$meta])*
        $vis fn $name(_ctx: &mut $crate::arch::syscall::SyscallFrame) -> $crate::syscalls::SyscallResult {
            fn inner($($($arg)+ : $arg_ty),*) -> $ret $body

            fn inner_wrapper(_ctx: &mut $crate::arch::syscall::SyscallFrame) -> $crate::syscalls::SyscallResult {
                let ($(${ignore($arg_ty)} $crate::paste::paste! { [< syscall_arg ${index(0)} >] },)*) = (
                    $(
                        $crate::paste::paste! {
                            <$arg_ty as $crate::syscalls::SyscallArg>::from_arg(
                                _ctx.nth_arg(${index(0)})
                            )?
                        },
                    )*
                );

                let result = inner($(${ignore($arg_ty)} $crate::paste::paste! { [< syscall_arg ${index(0)} >] }),*);
                $crate::syscalls::SyscallReturn::into_syscall_result(result)
            }

            inner_wrapper(_ctx)
        }
    };

    // --- attr() entrypoints (nightly #![feature(macro_attr)]) ---
    attr() {
        $(#[$meta:meta])*
        $vis:vis fn $name:ident ( $($args:tt)* ) -> $ret:ty $body:block
    } => {
        $crate::wrap_syscall!(@expand $(#[$meta])* $vis fn $name($($args)*) -> $ret $body);
    };

    attr() {
        $(#[$meta:meta])*
        $vis:vis fn $name:ident ( $($args:tt)* ) $body:block
    } => {
        $crate::wrap_syscall!(@expand $(#[$meta])* $vis fn $name($($args)*) -> () $body);
    };

    // --- functional entrypoints ---
    (
        $(#[$meta:meta])*
        $vis:vis fn $name:ident ( $($args:tt)* ) -> $ret:ty $body:block
    ) => {
        $crate::wrap_syscall!(@expand $(#[$meta])* $vis fn $name($($args)*) -> $ret $body);
    };

    (
        $(#[$meta:meta])*
        $vis:vis fn $name:ident ( $($args:tt)* ) $body:block
    ) => {
        $crate::wrap_syscall!(@expand $(#[$meta])* $vis fn $name($($args)*) -> () $body);
    };
}

/// Helper macro to adapt an existing strongly-typed function into a `SyscallHandler` closure.
#[macro_export]
macro_rules! syscall_adapter {
    ($func:path, ($($arg_ty:ty),* $(,)?)) => {
        |frame: &mut $crate::arch::syscall::SyscallFrame| -> $crate::syscalls::SyscallResult {
            let ($(${ignore($arg_ty)} $crate::paste::paste! { [< syscall_arg ${index(0)} >] },)*) = (
                $(
                    $crate::paste::paste! {
                        <$arg_ty as $crate::syscalls::SyscallArg>::from_arg(
                            frame.nth_arg(${index(0)})
                        )?
                    },
                )*
            );
            let result = $func($(${ignore($arg_ty)} $crate::paste::paste! { [< syscall_arg ${index(0)} >] }),*);
            $crate::syscalls::SyscallReturn::into_syscall_result(result)
        }
    };
}

/// Helper macro for unimplemented syscalls.
#[macro_export]
macro_rules! sys_unimpl {
    ($name:expr) => {{
        fn unimp(
            _frame: &mut $crate::arch::syscall::SyscallFrame,
        ) -> $crate::syscalls::SyscallResult {
            log::warn!("Call to unimplemented syscall: {}", $name);
            Err($crate::syscalls::SyscallError::ENOSYS)
        }
        unimp
    }};
    ($name:expr, $ret:expr) => {{
        fn unimp(
            _frame: &mut $crate::arch::syscall::SyscallFrame,
        ) -> $crate::syscalls::SyscallResult {
            log::warn!("Call to unimplemented syscall: {}", $name);
            $ret
        }
        unimp
    }};
}

/// Unified macro to define architecture-specific syscall numbers and construct the static dispatch table.
/// Supports raw handlers (`SyscallHandler`), wrapped functions (`wrap handler, (args...)`), and functions defined via `#[wrap_syscall]`.
#[macro_export]
macro_rules! define_syscall_table {
    (@entry $num:expr, $name:expr, wrap $handler:path, ($($arg_ty:ty),*)) => {
        $crate::syscalls::SyscallEntry {
            num: $num,
            name: $name,
            handler: $crate::syscall_adapter!($handler, ($($arg_ty),*)),
        }
    };
    (@entry $num:expr, $name:expr, $handler:expr) => {
        $crate::syscalls::SyscallEntry {
            num: $num,
            name: $name,
            handler: $handler,
        }
    };
    ($( $const_name:ident = $num:expr => ($name:expr, $($entry:tt)+) ),* $(,)?) => {
        pub static SYSCALL_TABLE: &[$crate::syscalls::SyscallEntry] = &[
            $(
                $crate::define_syscall_table!(@entry $num, $name, $($entry)+),
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
            (entry.handler)(frame)
        }
        Err(_) => {
            log::warn!("Unhandled system call nr: {}", sys_num);
            Err(SyscallError::ENOSYS)
        }
    };

    result.into_raw()
}
