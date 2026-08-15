pub mod fs;
pub mod ioctl;
pub mod mm;
pub mod proc;
pub mod signals;
pub mod sys_info;
pub mod time;

use crate::arch::syscall::SyscallFrame;

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
    EEXIST = 17,
    ENOTDIR = 20,
    EISDIR = 21,
    EINVAL = 22,
    EMFILE = 24,
    ENOTTY = 25,
    ENOSYS = 38,
}

impl From<crate::fs::vfs::types::VfsError> for SyscallError {
    fn from(err: crate::fs::vfs::types::VfsError) -> Self {
        use crate::fs::vfs::types::VfsError;
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

/// Maximum virtual address allowed for user space pointers (Ring 3 canonical boundary).
pub const USER_SPACE_MAX_ADDR: u64 = 0x0000_7FFF_FFFF_FFFF;

/// Validate if a user pointer and range lies strictly within user-space memory.
pub fn is_user_ptr_valid(ptr: u64, len: usize) -> bool {
    if ptr == 0 {
        return false;
    }
    let end = match ptr.checked_add(len as u64) {
        Some(e) => e,
        None => return false,
    };
    end <= USER_SPACE_MAX_ADDR
}

/// Helper to safely copy a null-terminated string from user space memory into kernel `String`.
pub unsafe fn read_user_string(
    ptr: *const u8,
    max_len: usize,
) -> Result<alloc::string::String, SyscallError> {
    if !is_user_ptr_valid(ptr as u64, 1) {
        return Err(SyscallError::EFAULT);
    }
    let mut vec = alloc::vec::Vec::new();
    for i in 0..max_len {
        let addr = ptr as u64 + i as u64;
        if !is_user_ptr_valid(addr, 1) {
            return Err(SyscallError::EFAULT);
        }
        // SAFETY: User space memory range validated.
        let byte = unsafe { core::ptr::read_volatile((addr) as *const u8) };
        if byte == 0 {
            break;
        }
        vec.push(byte);
    }
    alloc::string::String::from_utf8(vec).map_err(|_| SyscallError::EINVAL)
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
