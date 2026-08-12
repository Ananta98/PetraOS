pub mod fs;
pub mod ioctl;
pub mod mm;
pub mod proc;
pub mod signals;
pub mod sys_info;
pub mod time;

use crate::arch::syscall::syscall::SyscallFrame;

/// Standard x86_64 System Call Numbers
pub const SYS_READ: u64 = 0;
pub const SYS_WRITE: u64 = 1;
pub const SYS_OPEN: u64 = 2;
pub const SYS_CLOSE: u64 = 3;
pub const SYS_STAT: u64 = 4;
pub const SYS_FSTAT: u64 = 5;
pub const SYS_LSEEK: u64 = 8;
pub const SYS_MMAP: u64 = 9;
pub const SYS_MPROTECT: u64 = 10;
pub const SYS_MUNMAP: u64 = 11;
pub const SYS_BRK: u64 = 12;
pub const SYS_RT_SIGACTION: u64 = 13;
pub const SYS_RT_SIGPROCMASK: u64 = 14;
pub const SYS_RT_SIGRETURN: u64 = 15;
pub const SYS_IOCTL: u64 = 16;
pub const SYS_PIPE: u64 = 22;
pub const SYS_YIELD: u64 = 24;
pub const SYS_DUP: u64 = 32;
pub const SYS_DUP2: u64 = 33;
pub const SYS_GETPID: u64 = 39;
pub const SYS_FORK: u64 = 57;
pub const SYS_VFORK: u64 = 58;
pub const SYS_EXECVE: u64 = 59;
pub const SYS_EXIT: u64 = 60;
pub const SYS_WAIT4: u64 = 61;
pub const SYS_KILL: u64 = 62;
pub const SYS_UNAME: u64 = 63;
pub const SYS_FCNTL: u64 = 72;
pub const SYS_GETCWD: u64 = 79;
pub const SYS_CHDIR: u64 = 80;
pub const SYS_GETTIMEOFDAY: u64 = 96;
pub const SYS_TIMES: u64 = 100;
pub const SYS_SETPGID: u64 = 109;
pub const SYS_GETPPID: u64 = 110;
pub const SYS_GETPGRP: u64 = 111;
pub const SYS_ISATTY: u64 = 215;
pub const SYS_EXIT_GROUP: u64 = 231;
pub const SYS_OPENAT: u64 = 257;
pub const SYS_NEWFSTATAT: u64 = 262;
pub const SYS_DUP3: u64 = 292;
pub const SYS_PIPE2: u64 = 293;

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
    SYS_READ => ("read", fs::sys_read),
    SYS_WRITE => ("write", fs::sys_write),
    SYS_OPEN => ("open", fs::sys_open),
    SYS_CLOSE => ("close", fs::sys_close),
    SYS_STAT => ("stat", fs::sys_stat),
    SYS_FSTAT => ("fstat", fs::sys_fstat),
    SYS_LSEEK => ("lseek", fs::sys_lseek),
    SYS_MMAP => ("mmap", mm::sys_mmap),
    SYS_MPROTECT => ("mprotect", mm::sys_mprotect),
    SYS_MUNMAP => ("munmap", mm::sys_munmap),
    SYS_BRK => ("brk", mm::sys_brk),
    SYS_RT_SIGACTION => ("rt_sigaction", signals::sys_rt_sigaction),
    SYS_RT_SIGPROCMASK => ("rt_sigprocmask", signals::sys_rt_sigprocmask),
    SYS_RT_SIGRETURN => ("rt_sigreturn", signals::sys_rt_sigreturn),
    SYS_IOCTL => ("ioctl", ioctl::sys_ioctl),
    SYS_PIPE => ("pipe", fs::sys_pipe),
    SYS_YIELD => ("yield", proc::sys_yield),
    SYS_DUP => ("dup", fs::sys_dup),
    SYS_DUP2 => ("dup2", fs::sys_dup2),
    SYS_GETPID => ("getpid", proc::sys_getpid),
    SYS_FORK => ("fork", proc::sys_fork),
    SYS_VFORK => ("vfork", proc::sys_vfork),
    SYS_EXECVE => ("execve", proc::sys_execve),
    SYS_EXIT => ("exit", proc::sys_exit),
    SYS_WAIT4 => ("wait4", proc::sys_wait4),
    SYS_KILL => ("kill", signals::sys_kill),
    SYS_UNAME => ("uname", sys_info::sys_uname),
    SYS_FCNTL => ("fcntl", fs::sys_fcntl),
    SYS_GETCWD => ("getcwd", fs::sys_getcwd),
    SYS_CHDIR => ("chdir", fs::sys_chdir),
    SYS_GETTIMEOFDAY => ("gettimeofday", time::sys_gettimeofday),
    SYS_TIMES => ("times", time::sys_times),
    SYS_SETPGID => ("setpgid", proc::sys_setpgid),
    SYS_GETPPID => ("getppid", proc::sys_getppid),
    SYS_GETPGRP => ("getpgrp", proc::sys_getpgrp),
    SYS_ISATTY => ("isatty", ioctl::sys_isatty),
    SYS_EXIT_GROUP => ("exit_group", proc::sys_exit_group),
    SYS_OPENAT => ("openat", fs::sys_openat),
    SYS_NEWFSTATAT => ("newfstatat", fs::sys_newfstatat),
    SYS_DUP3 => ("dup3", fs::sys_dup3),
    SYS_PIPE2 => ("pipe2", fs::sys_pipe2),
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
