use super::{SyscallError, SyscallResult, is_user_ptr_valid, read_user_string};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::File;
use crate::fs::vfs::types::{InodeType, LinuxStat, O_CREAT, O_RDONLY, O_WRONLY, SeekWhence, Stat};
use alloc::string::String;
use alloc::sync::Arc;

pub const AT_FDCWD: i32 = -100;

pub const O_CLOEXEC: u32 = 0x80000;
pub const O_NONBLOCK: u32 = 0x800;

pub const F_DUPFD: i32 = 0;
pub const F_GETFD: i32 = 1;
pub const F_SETFD: i32 = 2;
pub const F_GETFL: i32 = 3;
pub const F_SETFL: i32 = 4;
pub const F_GETLK: i32 = 5;
pub const F_SETLK: i32 = 6;
pub const F_SETLKW: i32 = 7;
pub const F_SETOWN: i32 = 8;
pub const F_GETOWN: i32 = 9;
pub const F_DUPFD_CLOEXEC: i32 = 1030;

pub const DT_UNKNOWN: u8 = 0;
pub const DT_FIFO: u8 = 1;
pub const DT_CHR: u8 = 2;
pub const DT_DIR: u8 = 4;
pub const DT_BLK: u8 = 6;
pub const DT_REG: u8 = 8;
pub const DT_LNK: u8 = 10;
pub const DT_SOCK: u8 = 12;

/// `sys_read` (SYS_READ = 0)
/// Read from a file descriptor.
pub fn sys_read(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let buf = frame.arg2() as *mut u8;
    let count = frame.arg3() as usize;

    if fd < 0 {
        return Err(SyscallError::EBADF);
    }
    if count == 0 {
        return Ok(0);
    }
    if !is_user_ptr_valid(buf as u64, count) {
        return Err(SyscallError::EFAULT);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();

    let file = proc.fd_table.get(fd)?;
    drop(proc);

    // SAFETY: User buffer pointer range validated within user space bounds.
    let user_slice = unsafe { core::slice::from_raw_parts_mut(buf, count) };
    let bytes_read = file.read(user_slice)?;
    Ok(bytes_read)
}

/// `sys_write` (SYS_WRITE = 1)
/// Write to a file descriptor.
pub fn sys_write(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let buf = frame.arg2() as *const u8;
    let count = frame.arg3() as usize;

    if fd < 0 {
        return Err(SyscallError::EBADF);
    }
    if count == 0 {
        return Ok(0);
    }
    if !is_user_ptr_valid(buf as u64, count) {
        return Err(SyscallError::EFAULT);
    }

    // SAFETY: User buffer pointer range validated within user space bounds.
    let user_slice = unsafe { core::slice::from_raw_parts(buf, count) };

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();

    let file = proc.fd_table.get(fd)?;
    drop(proc);

    let bytes_written = file.write(user_slice)?;
    Ok(bytes_written)
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct IoVec {
    pub iov_base: u64,
    pub iov_len: usize,
}

pub const POLLIN: i16 = 0x0001;
pub const POLLPRI: i16 = 0x0002;
pub const POLLOUT: i16 = 0x0004;
pub const POLLERR: i16 = 0x0008;
pub const POLLHUP: i16 = 0x0010;
pub const POLLNVAL: i16 = 0x0020;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PollFd {
    pub fd: i32,
    pub events: i16,
    pub revents: i16,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct LinuxTimespec {
    pub tv_sec: i64,
    pub tv_nsec: i64,
}

pub const FD_SETSIZE: usize = 1024;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FdSet {
    pub fds_bits: [u64; FD_SETSIZE / 64],
}

pub fn resolve_at_path(dfd: i32, path: &str) -> Result<String, SyscallError> {
    if path.starts_with('/') {
        Ok(crate::fs::normalize_path("/", path))
    } else if dfd == AT_FDCWD || dfd as u32 == 0xffffff9c {
        let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
        let proc = proc_arc.lock();
        Ok(crate::fs::normalize_path(&proc.cwd, path))
    } else if dfd >= 0 {
        let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
        let proc = proc_arc.lock();
        let file = proc.fd_table.get(dfd)?;
        let dir_path = crate::fs::build_path(&file.dentry);
        Ok(crate::fs::normalize_path(&dir_path, path))
    } else {
        let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
        let proc = proc_arc.lock();
        Ok(crate::fs::normalize_path(&proc.cwd, path))
    }
}

fn do_openat(dfd: i32, path: &str, flags: u32) -> SyscallResult {
    let full_path = resolve_at_path(dfd, path)?;

    let dentry = match crate::fs::resolve_path(&full_path) {
        Ok(d) => {
            if (flags & crate::fs::O_CREAT) != 0 && (flags & crate::fs::O_EXCL) != 0 {
                return Err(SyscallError::EEXIST);
            }
            if (flags & crate::fs::O_DIRECTORY) != 0 && d.inode.inode_type != InodeType::Directory {
                return Err(SyscallError::ENOTDIR);
            }
            if d.inode.inode_type == InodeType::Directory && (crate::fs::can_write(flags)) {
                return Err(SyscallError::EISDIR);
            }
            if (flags & crate::fs::O_TRUNC) != 0 && crate::fs::can_write(flags) {
                let _ = d.inode.ops.truncate(0);
            }
            d
        }
        Err(crate::fs::vfs::types::VfsError::NotFound) if (flags & crate::fs::O_CREAT) != 0 => {
            if (flags & crate::fs::O_DIRECTORY) != 0 {
                return Err(SyscallError::ENOENT);
            }
            crate::fs::create_file(&full_path)?
        }
        Err(err) => return Err(SyscallError::from(err)),
    };

    let file_ops = dentry.inode.ops.open()?;
    let file = Arc::new(File::new(dentry, flags, file_ops));

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let fd = proc.fd_table.alloc(file);

    Ok(fd as usize)
}

/// `sys_open` (SYS_OPEN = 2)
/// Open a file.
pub fn sys_open(frame: &mut SyscallFrame) -> SyscallResult {
    let path_ptr = frame.arg1() as *const u8;
    let flags = frame.arg2() as u32;

    let path = unsafe { read_user_string(path_ptr, 256)? };
    do_openat(AT_FDCWD, &path, flags)
}

/// `sys_openat` (SYS_OPENAT = 257)
/// Open a file relative to directory descriptor.
pub fn sys_openat(frame: &mut SyscallFrame) -> SyscallResult {
    let dfd = frame.arg1() as i32;
    let path_ptr = frame.arg2() as *const u8;
    let flags = frame.arg3() as u32;

    let path = unsafe { read_user_string(path_ptr, 256)? };
    do_openat(dfd, &path, flags)
}

/// `sys_close` (SYS_CLOSE = 3)
/// Close a file descriptor.
pub fn sys_close(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    if fd < 0 {
        return Err(SyscallError::EBADF);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    proc.fd_table.close(fd)?;

    Ok(0)
}

/// `sys_stat` (SYS_STAT = 4)
/// Get file status by path.
pub fn sys_stat(frame: &mut SyscallFrame) -> SyscallResult {
    let path_ptr = frame.arg1() as *const u8;
    let statbuf = frame.arg2() as *mut LinuxStat;

    if !is_user_ptr_valid(statbuf as u64, core::mem::size_of::<LinuxStat>()) {
        return Err(SyscallError::EFAULT);
    }

    let path = unsafe { read_user_string(path_ptr, 256)? };
    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    let vfs_stat = crate::fs::stat(&full_path)?;

    let linux_stat = copy_to_linux_stat(&vfs_stat);
    // SAFETY: Writing stat struct to user statbuf after validation.
    unsafe {
        core::ptr::write_volatile(statbuf, linux_stat);
    }

    Ok(0)
}

/// `sys_fstat` (SYS_FSTAT = 5)
/// Get file status by descriptor.
pub fn sys_fstat(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let statbuf = frame.arg2() as *mut LinuxStat;

    if fd < 0 {
        return Err(SyscallError::EBADF);
    }
    if !is_user_ptr_valid(statbuf as u64, core::mem::size_of::<LinuxStat>()) {
        return Err(SyscallError::EFAULT);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(fd)?;
    drop(proc);

    let vfs_stat = file.ops.stat().or_else(|_| file.dentry.inode.ops.stat())?;
    let linux_stat = copy_to_linux_stat(&vfs_stat);

    // SAFETY: Writing stat struct to user statbuf after validation.
    unsafe {
        core::ptr::write_volatile(statbuf, linux_stat);
    }

    Ok(0)
}

/// `sys_lseek` (SYS_LSEEK = 8)
/// Reposition read/write file offset.
pub fn sys_lseek(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let offset = frame.arg2() as i64;
    let whence_raw = frame.arg3() as i32;

    if fd < 0 {
        return Err(SyscallError::EBADF);
    }

    let whence = match whence_raw {
        0 => SeekWhence::Set,
        1 => SeekWhence::Cur,
        2 => SeekWhence::End,
        _ => return Err(SyscallError::EINVAL),
    };

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(fd)?;
    drop(proc);

    let new_offset = match file.lseek(offset, whence) {
        Ok(off) => off,
        Err(crate::fs::vfs::types::VfsError::NotSupported) => return Err(SyscallError::ESPIPE),
        Err(e) => return Err(e.into()),
    };
    Ok(new_offset)
}

/// `sys_dup` (SYS_DUP = 32)
/// Duplicate an open file descriptor.
pub fn sys_dup(frame: &mut SyscallFrame) -> SyscallResult {
    let oldfd = frame.arg1() as i32;
    if oldfd < 0 {
        return Err(SyscallError::EBADF);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(oldfd)?;
    let newfd = proc.fd_table.alloc(file);

    Ok(newfd as usize)
}

/// `sys_dup2` (SYS_DUP2 = 33)
/// Duplicate a file descriptor onto a specified target descriptor.
pub fn sys_dup2(frame: &mut SyscallFrame) -> SyscallResult {
    let oldfd = frame.arg1() as i32;
    let newfd = frame.arg2() as i32;

    if oldfd < 0 || newfd < 0 {
        return Err(SyscallError::EBADF);
    }
    if oldfd == newfd {
        return Ok(newfd as usize);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(oldfd)?;
    proc.fd_table.set_with_flags(newfd, file, 0)?;

    Ok(newfd as usize)
}

/// `sys_dup3` (SYS_DUP3 = 292)
/// Duplicate a file descriptor with flags.
pub fn sys_dup3(frame: &mut SyscallFrame) -> SyscallResult {
    let oldfd = frame.arg1() as i32;
    let newfd = frame.arg2() as i32;
    let flags = frame.arg3() as u32;

    if oldfd == newfd || oldfd < 0 || newfd < 0 {
        return Err(SyscallError::EINVAL);
    }

    let cloexec = if (flags & O_CLOEXEC) != 0 {
        crate::fs::fd::FD_CLOEXEC
    } else {
        0
    };

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(oldfd)?;
    proc.fd_table.set_with_flags(newfd, file, cloexec)?;

    Ok(newfd as usize)
}

/// `sys_pipe` (SYS_PIPE = 22)
/// Create an anonymous inter-process pipe.
pub fn sys_pipe(frame: &mut SyscallFrame) -> SyscallResult {
    let pipefd = frame.arg1() as *mut i32;

    if !is_user_ptr_valid(pipefd as u64, 2 * core::mem::size_of::<i32>()) {
        return Err(SyscallError::EFAULT);
    }

    let (f_read, f_write) = crate::fs::pipe::create_pipe(false)?;

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();

    let r_fd = proc.fd_table.alloc(f_read);
    let w_fd = proc.fd_table.alloc(f_write);

    // SAFETY: User pipefd pointer range validated within Ring 3 address bounds.
    unsafe {
        core::ptr::write_volatile(pipefd, r_fd);
        core::ptr::write_volatile(pipefd.add(1), w_fd);
    }

    Ok(0)
}

/// `sys_pipe2` (SYS_PIPE2 = 293)
/// Create an anonymous pipe with flags.
pub fn sys_pipe2(frame: &mut SyscallFrame) -> SyscallResult {
    let pipefd = frame.arg1() as *mut i32;
    let flags = frame.arg2() as u32;

    if !is_user_ptr_valid(pipefd as u64, 2 * core::mem::size_of::<i32>()) {
        return Err(SyscallError::EFAULT);
    }

    let nonblocking = (flags & O_NONBLOCK) != 0;
    let cloexec = if (flags & O_CLOEXEC) != 0 {
        crate::fs::fd::FD_CLOEXEC
    } else {
        0
    };

    let (f_read, f_write) = crate::fs::pipe::create_pipe(nonblocking)?;

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();

    let r_fd = proc.fd_table.alloc_with_flags(f_read, cloexec);
    let w_fd = proc.fd_table.alloc_with_flags(f_write, cloexec);

    // SAFETY: User pipefd pointer range validated within Ring 3 address bounds.
    unsafe {
        core::ptr::write_volatile(pipefd, r_fd);
        core::ptr::write_volatile(pipefd.add(1), w_fd);
    }

    Ok(0)
}

/// `sys_getcwd` (SYS_GETCWD = 79)
/// Get current working directory string.
pub fn sys_getcwd(frame: &mut SyscallFrame) -> SyscallResult {
    let buf = frame.arg1() as *mut u8;
    let size = frame.arg2() as usize;

    if size == 0 || !is_user_ptr_valid(buf as u64, size) {
        return Err(SyscallError::EINVAL);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();

    let cwd_bytes = proc.cwd.as_bytes();
    if cwd_bytes.len() + 1 > size {
        return Err(SyscallError::ENOMEM);
    }

    // SAFETY: User buffer pointer range validated within Ring 3 address bounds.
    unsafe {
        core::ptr::copy_nonoverlapping(cwd_bytes.as_ptr(), buf, cwd_bytes.len());
        core::ptr::write_volatile(buf.add(cwd_bytes.len()), 0);
    }

    Ok(buf as usize)
}

/// `sys_chdir` (SYS_CHDIR = 80)
/// Change working directory.
pub fn sys_chdir(frame: &mut SyscallFrame) -> SyscallResult {
    let path_ptr = frame.arg1() as *const u8;
    let path = unsafe { read_user_string(path_ptr, 256)? };

    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    let dentry = crate::fs::resolve_path(&full_path)?;
    if dentry.inode.inode_type != crate::fs::vfs::types::InodeType::Directory {
        return Err(SyscallError::ENOTDIR);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let mut proc = proc_arc.lock();
    proc.cwd = crate::fs::normalize_path(&proc.cwd, &full_path);

    Ok(0)
}

/// `sys_fcntl` (SYS_FCNTL = 72)
/// Manipulate file descriptor properties.
pub fn sys_fcntl(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let cmd = frame.arg2() as i32;
    let arg = frame.arg3();

    if fd < 0 {
        return Err(SyscallError::EBADF);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();

    match cmd {
        F_DUPFD => {
            let min_fd = arg as i32;
            if min_fd < 0 {
                return Err(SyscallError::EINVAL);
            }
            let file = proc.fd_table.get(fd)?;
            let new_fd = proc.fd_table.alloc_from(min_fd, file, 0)?;
            Ok(new_fd as usize)
        }
        F_DUPFD_CLOEXEC => {
            let min_fd = arg as i32;
            if min_fd < 0 {
                return Err(SyscallError::EINVAL);
            }
            let file = proc.fd_table.get(fd)?;
            let new_fd = proc
                .fd_table
                .alloc_from(min_fd, file, crate::fs::fd::FD_CLOEXEC)?;
            Ok(new_fd as usize)
        }
        F_GETFD => {
            let flags = proc.fd_table.get_flags(fd)?;
            Ok(flags as usize)
        }
        F_SETFD => {
            let flags = arg as u32;
            proc.fd_table.set_flags(fd, flags)?;
            Ok(0)
        }
        F_GETFL => {
            let file = proc.fd_table.get(fd)?;
            Ok(file.flags as usize)
        }
        F_SETFL => Ok(0),
        F_GETLK | F_SETLK | F_SETLKW | F_SETOWN | F_GETOWN => Ok(0),
        _ => Ok(0),
    }
}

/// `sys_access` (SYS_ACCESS = 21)
/// Check user's permissions for a file.
pub fn sys_access(frame: &mut SyscallFrame) -> SyscallResult {
    let path_ptr = frame.arg1() as *const u8;
    let mode = frame.arg2() as i32;

    if mode < 0 || mode > 7 {
        return Err(SyscallError::EINVAL);
    }

    let path = unsafe { read_user_string(path_ptr, 256)? };
    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    let _dentry = crate::fs::resolve_path(&full_path)?;

    Ok(0)
}

/// `sys_umask` (SYS_UMASK = 95)
/// Set file mode creation mask.
pub fn sys_umask(frame: &mut SyscallFrame) -> SyscallResult {
    let mask = frame.arg1() as u32;

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let mut proc = proc_arc.lock();
    let old_mask = proc.umask;
    proc.umask = mask & 0o777;

    Ok(old_mask as usize)
}

/// `sys_newfstatat` (SYS_NEWFSTATAT = 262)
/// Get file status relative to directory descriptor.
pub fn sys_newfstatat(frame: &mut SyscallFrame) -> SyscallResult {
    let dfd = frame.arg1() as i32;
    let path_ptr = frame.arg2() as *const u8;
    let statbuf = frame.arg3() as *mut LinuxStat;

    if !is_user_ptr_valid(statbuf as u64, core::mem::size_of::<LinuxStat>()) {
        return Err(SyscallError::EFAULT);
    }

    let path = unsafe { read_user_string(path_ptr, 256)? };
    let full_path = resolve_at_path(dfd, &path)?;
    let vfs_stat = crate::fs::stat(&full_path)?;

    let linux_stat = copy_to_linux_stat(&vfs_stat);
    // SAFETY: Writing stat struct to user statbuf after validation.
    unsafe {
        core::ptr::write_volatile(statbuf, linux_stat);
    }

    Ok(0)
}

/// `sys_lstat` (SYS_LSTAT = 6)
/// Get file status without following symlinks.
pub fn sys_lstat(frame: &mut SyscallFrame) -> SyscallResult {
    let path_ptr = frame.arg1() as *const u8;
    let statbuf = frame.arg2() as *mut LinuxStat;

    if !is_user_ptr_valid(statbuf as u64, core::mem::size_of::<LinuxStat>()) {
        return Err(SyscallError::EFAULT);
    }

    let path = unsafe { read_user_string(path_ptr, 256)? };
    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    let vfs_stat = crate::fs::lstat(&full_path)?;

    let linux_stat = copy_to_linux_stat(&vfs_stat);
    // SAFETY: Writing stat struct to user statbuf after validation.
    unsafe {
        core::ptr::write_volatile(statbuf, linux_stat);
    }

    Ok(0)
}

/// `sys_poll` (SYS_POLL = 7)
pub fn sys_poll(frame: &mut SyscallFrame) -> SyscallResult {
    let fds_ptr = frame.arg1() as *mut PollFd;
    let nfds = frame.arg2() as usize;
    let _timeout = frame.arg3() as i32;

    if nfds == 0 {
        return Ok(0);
    }
    if nfds > 1024 || !is_user_ptr_valid(fds_ptr as u64, nfds * core::mem::size_of::<PollFd>()) {
        return Err(SyscallError::EFAULT);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();

    let mut ready_count = 0;
    for i in 0..nfds {
        let pfd = unsafe { &mut *fds_ptr.add(i) };
        if pfd.fd < 0 {
            pfd.revents = 0;
            continue;
        }
        match proc.fd_table.get(pfd.fd) {
            Ok(file) => {
                let mut revents = 0;
                if (pfd.events & POLLIN) != 0 && crate::fs::can_read(file.flags) {
                    revents |= POLLIN;
                }
                if (pfd.events & POLLOUT) != 0 && crate::fs::can_write(file.flags) {
                    revents |= POLLOUT;
                }
                pfd.revents = revents;
                if revents != 0 {
                    ready_count += 1;
                }
            }
            Err(_) => {
                pfd.revents = POLLNVAL;
                ready_count += 1;
            }
        }
    }
    Ok(ready_count)
}

/// `sys_ppoll` (SYS_PPOLL = 271)
pub fn sys_ppoll(frame: &mut SyscallFrame) -> SyscallResult {
    sys_poll(frame)
}

/// `sys_readv` (SYS_READV = 19)
pub fn sys_readv(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let iov_ptr = frame.arg2() as *const IoVec;
    let iovcnt = frame.arg3() as usize;

    if fd < 0 {
        return Err(SyscallError::EBADF);
    }
    if iovcnt == 0 {
        return Ok(0);
    }
    if iovcnt > 1024 || !is_user_ptr_valid(iov_ptr as u64, iovcnt * core::mem::size_of::<IoVec>()) {
        return Err(SyscallError::EFAULT);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(fd)?;
    drop(proc);

    let mut total_read = 0;
    for i in 0..iovcnt {
        let iov = unsafe { *iov_ptr.add(i) };
        if iov.iov_len == 0 {
            continue;
        }
        if !is_user_ptr_valid(iov.iov_base, iov.iov_len) {
            return Err(SyscallError::EFAULT);
        }
        let user_slice =
            unsafe { core::slice::from_raw_parts_mut(iov.iov_base as *mut u8, iov.iov_len) };
        let n = file.read(user_slice)?;
        total_read += n;
        if n < iov.iov_len {
            break;
        }
    }
    Ok(total_read)
}

/// `sys_writev` (SYS_WRITEV = 20)
pub fn sys_writev(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let iov_ptr = frame.arg2() as *const IoVec;
    let iovcnt = frame.arg3() as usize;

    if fd < 0 {
        return Err(SyscallError::EBADF);
    }
    if iovcnt == 0 {
        return Ok(0);
    }
    if iovcnt > 1024 || !is_user_ptr_valid(iov_ptr as u64, iovcnt * core::mem::size_of::<IoVec>()) {
        return Err(SyscallError::EFAULT);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(fd)?;
    drop(proc);

    let mut total_written = 0;
    for i in 0..iovcnt {
        let iov = unsafe { *iov_ptr.add(i) };
        if iov.iov_len == 0 {
            continue;
        }
        if !is_user_ptr_valid(iov.iov_base, iov.iov_len) {
            return Err(SyscallError::EFAULT);
        }
        let user_slice =
            unsafe { core::slice::from_raw_parts(iov.iov_base as *const u8, iov.iov_len) };
        let n = file.write(user_slice)?;
        total_written += n;
        if n < iov.iov_len {
            break;
        }
    }
    Ok(total_written)
}

/// `sys_select` (SYS_SELECT = 23)
pub fn sys_select(frame: &mut SyscallFrame) -> SyscallResult {
    let nfds = frame.arg1() as i32;
    let readfds = frame.arg2() as *mut FdSet;
    let writefds = frame.arg3() as *mut FdSet;
    let exceptfds = frame.arg4() as *mut FdSet;
    let _timeout = frame.arg5() as *const LinuxTimespec;

    if nfds < 0 || nfds > FD_SETSIZE as i32 {
        return Err(SyscallError::EINVAL);
    }
    if nfds == 0 {
        return Ok(0);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();

    let mut ready_count = 0;

    for fd in 0..nfds {
        let word = (fd / 64) as usize;
        let bit = 1u64 << (fd % 64);

        let mut is_r = false;
        let mut is_w = false;

        if !readfds.is_null() && is_user_ptr_valid(readfds as u64, core::mem::size_of::<FdSet>()) {
            unsafe {
                if ((*readfds).fds_bits[word] & bit) != 0 {
                    is_r = true;
                }
            }
        }
        if !writefds.is_null() && is_user_ptr_valid(writefds as u64, core::mem::size_of::<FdSet>())
        {
            unsafe {
                if ((*writefds).fds_bits[word] & bit) != 0 {
                    is_w = true;
                }
            }
        }
        if !exceptfds.is_null()
            && is_user_ptr_valid(exceptfds as u64, core::mem::size_of::<FdSet>())
        {
            unsafe {
                (*exceptfds).fds_bits[word] &= !bit;
            }
        }

        if is_r || is_w {
            if let Ok(file) = proc.fd_table.get(fd) {
                if is_r && crate::fs::can_read(file.flags) {
                    ready_count += 1;
                } else if !readfds.is_null() {
                    unsafe {
                        (*readfds).fds_bits[word] &= !bit;
                    }
                }
                if is_w && crate::fs::can_write(file.flags) {
                    ready_count += 1;
                } else if !writefds.is_null() {
                    unsafe {
                        (*writefds).fds_bits[word] &= !bit;
                    }
                }
            } else {
                if !readfds.is_null() {
                    unsafe {
                        (*readfds).fds_bits[word] &= !bit;
                    }
                }
                if !writefds.is_null() {
                    unsafe {
                        (*writefds).fds_bits[word] &= !bit;
                    }
                }
            }
        }
    }

    Ok(ready_count)
}

/// `sys_pselect6` (SYS_PSELECT6 = 270)
pub fn sys_pselect6(frame: &mut SyscallFrame) -> SyscallResult {
    sys_select(frame)
}

/// `sys_fsync` (SYS_FSYNC = 74)
pub fn sys_fsync(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    if fd < 0 {
        return Err(SyscallError::EBADF);
    }
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(fd)?;
    drop(proc);
    file.ops.sync()?;
    Ok(0)
}

/// `sys_fdatasync` (SYS_FDATASYNC = 75)
pub fn sys_fdatasync(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    if fd < 0 {
        return Err(SyscallError::EBADF);
    }
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(fd)?;
    drop(proc);
    file.ops.sync()?;
    Ok(0)
}

/// `sys_truncate` (SYS_TRUNCATE = 76)
pub fn sys_truncate(frame: &mut SyscallFrame) -> SyscallResult {
    let path_ptr = frame.arg1() as *const u8;
    let length = frame.arg2() as usize;

    let path = unsafe { read_user_string(path_ptr, 256)? };
    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    crate::fs::truncate(&full_path, length)?;
    Ok(0)
}

/// `sys_ftruncate` (SYS_FTRUNCATE = 77)
pub fn sys_ftruncate(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let length = frame.arg2() as usize;

    if fd < 0 {
        return Err(SyscallError::EBADF);
    }
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(fd)?;
    drop(proc);

    file.ops
        .truncate(length)
        .or_else(|_| file.dentry.inode.ops.truncate(length))?;
    Ok(0)
}

/// `sys_rename` (SYS_RENAME = 82)
pub fn sys_rename(frame: &mut SyscallFrame) -> SyscallResult {
    let old_ptr = frame.arg1() as *const u8;
    let new_ptr = frame.arg2() as *const u8;

    let old_path = unsafe { read_user_string(old_ptr, 256)? };
    let new_path = unsafe { read_user_string(new_ptr, 256)? };
    let old_full = resolve_at_path(AT_FDCWD, &old_path)?;
    let new_full = resolve_at_path(AT_FDCWD, &new_path)?;
    crate::fs::rename(&old_full, &new_full)?;
    Ok(0)
}

/// `sys_renameat` (SYS_RENAMEAT = 264)
pub fn sys_renameat(frame: &mut SyscallFrame) -> SyscallResult {
    let olddfd = frame.arg1() as i32;
    let old_ptr = frame.arg2() as *const u8;
    let newdfd = frame.arg3() as i32;
    let new_ptr = frame.arg4() as *const u8;

    let old_path = unsafe { read_user_string(old_ptr, 256)? };
    let new_path = unsafe { read_user_string(new_ptr, 256)? };
    let old_full = resolve_at_path(olddfd, &old_path)?;
    let new_full = resolve_at_path(newdfd, &new_path)?;
    crate::fs::rename(&old_full, &new_full)?;
    Ok(0)
}

/// `sys_renameat2` (SYS_RENAMEAT2 = 316)
pub fn sys_renameat2(frame: &mut SyscallFrame) -> SyscallResult {
    let olddfd = frame.arg1() as i32;
    let old_ptr = frame.arg2() as *const u8;
    let newdfd = frame.arg3() as i32;
    let new_ptr = frame.arg4() as *const u8;
    let _flags = frame.arg5() as u32;

    let old_path = unsafe { read_user_string(old_ptr, 256)? };
    let new_path = unsafe { read_user_string(new_ptr, 256)? };
    let old_full = resolve_at_path(olddfd, &old_path)?;
    let new_full = resolve_at_path(newdfd, &new_path)?;
    crate::fs::rename(&old_full, &new_full)?;
    Ok(0)
}

/// `sys_mkdir` (SYS_MKDIR = 83)
pub fn sys_mkdir(frame: &mut SyscallFrame) -> SyscallResult {
    let path_ptr = frame.arg1() as *const u8;
    let _mode = frame.arg2() as u32;

    let path = unsafe { read_user_string(path_ptr, 256)? };
    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    crate::fs::mkdir(&full_path)?;
    Ok(0)
}

/// `sys_mkdirat` (SYS_MKDIRAT = 258)
pub fn sys_mkdirat(frame: &mut SyscallFrame) -> SyscallResult {
    let dfd = frame.arg1() as i32;
    let path_ptr = frame.arg2() as *const u8;
    let _mode = frame.arg3() as u32;

    let path = unsafe { read_user_string(path_ptr, 256)? };
    let full_path = resolve_at_path(dfd, &path)?;
    crate::fs::mkdir(&full_path)?;
    Ok(0)
}

/// `sys_rmdir` (SYS_RMDIR = 84)
pub fn sys_rmdir(frame: &mut SyscallFrame) -> SyscallResult {
    let path_ptr = frame.arg1() as *const u8;
    let path = unsafe { read_user_string(path_ptr, 256)? };
    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    crate::fs::rmdir(&full_path)?;
    Ok(0)
}

/// `sys_link` (SYS_LINK = 86)
pub fn sys_link(frame: &mut SyscallFrame) -> SyscallResult {
    let old_ptr = frame.arg1() as *const u8;
    let new_ptr = frame.arg2() as *const u8;

    let old_path = unsafe { read_user_string(old_ptr, 256)? };
    let new_path = unsafe { read_user_string(new_ptr, 256)? };
    let old_full = resolve_at_path(AT_FDCWD, &old_path)?;
    let new_full = resolve_at_path(AT_FDCWD, &new_path)?;
    crate::fs::link(&old_full, &new_full)?;
    Ok(0)
}

/// `sys_linkat` (SYS_LINKAT = 265)
pub fn sys_linkat(frame: &mut SyscallFrame) -> SyscallResult {
    let olddfd = frame.arg1() as i32;
    let old_ptr = frame.arg2() as *const u8;
    let newdfd = frame.arg3() as i32;
    let new_ptr = frame.arg4() as *const u8;
    let _flags = frame.arg5() as i32;

    let old_path = unsafe { read_user_string(old_ptr, 256)? };
    let new_path = unsafe { read_user_string(new_ptr, 256)? };
    let old_full = resolve_at_path(olddfd, &old_path)?;
    let new_full = resolve_at_path(newdfd, &new_path)?;
    crate::fs::link(&old_full, &new_full)?;
    Ok(0)
}

/// `sys_unlink` (SYS_UNLINK = 87)
pub fn sys_unlink(frame: &mut SyscallFrame) -> SyscallResult {
    let path_ptr = frame.arg1() as *const u8;
    let path = unsafe { read_user_string(path_ptr, 256)? };
    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    crate::fs::unlink(&full_path)?;
    Ok(0)
}

/// `sys_unlinkat` (SYS_UNLINKAT = 263)
pub fn sys_unlinkat(frame: &mut SyscallFrame) -> SyscallResult {
    let dfd = frame.arg1() as i32;
    let path_ptr = frame.arg2() as *const u8;
    let flags = frame.arg3() as i32;

    let path = unsafe { read_user_string(path_ptr, 256)? };
    let full_path = resolve_at_path(dfd, &path)?;
    if (flags & crate::fs::AT_REMOVEDIR) != 0 {
        crate::fs::rmdir(&full_path)?;
    } else {
        crate::fs::unlink(&full_path)?;
    }
    Ok(0)
}

/// `sys_symlink` (SYS_SYMLINK = 88)
pub fn sys_symlink(frame: &mut SyscallFrame) -> SyscallResult {
    let target_ptr = frame.arg1() as *const u8;
    let link_ptr = frame.arg2() as *const u8;

    let target = unsafe { read_user_string(target_ptr, 256)? };
    let link_path = unsafe { read_user_string(link_ptr, 256)? };
    let full_path = resolve_at_path(AT_FDCWD, &link_path)?;
    crate::fs::symlink(&full_path, &target)?;
    Ok(0)
}

/// `sys_symlinkat` (SYS_SYMLINKAT = 266)
pub fn sys_symlinkat(frame: &mut SyscallFrame) -> SyscallResult {
    let target_ptr = frame.arg1() as *const u8;
    let newdfd = frame.arg2() as i32;
    let link_ptr = frame.arg3() as *const u8;

    let target = unsafe { read_user_string(target_ptr, 256)? };
    let link_path = unsafe { read_user_string(link_ptr, 256)? };
    let full_path = resolve_at_path(newdfd, &link_path)?;
    crate::fs::symlink(&full_path, &target)?;
    Ok(0)
}

/// `sys_readlink` (SYS_READLINK = 89)
pub fn sys_readlink(frame: &mut SyscallFrame) -> SyscallResult {
    let path_ptr = frame.arg1() as *const u8;
    let buf = frame.arg2() as *mut u8;
    let bufsiz = frame.arg3() as usize;

    if bufsiz == 0 || !is_user_ptr_valid(buf as u64, bufsiz) {
        return Err(SyscallError::EINVAL);
    }

    let path = unsafe { read_user_string(path_ptr, 256)? };
    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    let target = crate::fs::readlink(&full_path)?;
    let target_bytes = target.as_bytes();
    let copy_len = core::cmp::min(target_bytes.len(), bufsiz);

    // SAFETY: Validated user memory buffer.
    unsafe {
        core::ptr::copy_nonoverlapping(target_bytes.as_ptr(), buf, copy_len);
    }
    Ok(copy_len)
}

/// `sys_readlinkat` (SYS_READLINKAT = 267)
pub fn sys_readlinkat(frame: &mut SyscallFrame) -> SyscallResult {
    let dfd = frame.arg1() as i32;
    let path_ptr = frame.arg2() as *const u8;
    let buf = frame.arg3() as *mut u8;
    let bufsiz = frame.arg4() as usize;

    if bufsiz == 0 || !is_user_ptr_valid(buf as u64, bufsiz) {
        return Err(SyscallError::EINVAL);
    }

    let path = unsafe { read_user_string(path_ptr, 256)? };
    let full_path = resolve_at_path(dfd, &path)?;
    let target = crate::fs::readlink(&full_path)?;
    let target_bytes = target.as_bytes();
    let copy_len = core::cmp::min(target_bytes.len(), bufsiz);

    // SAFETY: Validated user memory buffer.
    unsafe {
        core::ptr::copy_nonoverlapping(target_bytes.as_ptr(), buf, copy_len);
    }
    Ok(copy_len)
}

/// `sys_chmod` (SYS_CHMOD = 90)
pub fn sys_chmod(frame: &mut SyscallFrame) -> SyscallResult {
    let path_ptr = frame.arg1() as *const u8;
    let mode = frame.arg2() as u32;

    let path = unsafe { read_user_string(path_ptr, 256)? };
    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    crate::fs::chmod(&full_path, mode)?;
    Ok(0)
}

/// `sys_fchmod` (SYS_FCHMOD = 91)
pub fn sys_fchmod(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let mode = frame.arg2() as u32;

    if fd < 0 {
        return Err(SyscallError::EBADF);
    }
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(fd)?;
    drop(proc);

    file.ops
        .chmod(mode)
        .or_else(|_| file.dentry.inode.ops.chmod(mode))?;
    Ok(0)
}

/// `sys_fchmodat` (SYS_FCHMODAT = 268)
pub fn sys_fchmodat(frame: &mut SyscallFrame) -> SyscallResult {
    let dfd = frame.arg1() as i32;
    let path_ptr = frame.arg2() as *const u8;
    let mode = frame.arg3() as u32;
    let _flags = frame.arg4() as i32;

    let path = unsafe { read_user_string(path_ptr, 256)? };
    let full_path = resolve_at_path(dfd, &path)?;
    crate::fs::chmod(&full_path, mode)?;
    Ok(0)
}

/// `sys_chown` (SYS_CHOWN = 92)
pub fn sys_chown(frame: &mut SyscallFrame) -> SyscallResult {
    let path_ptr = frame.arg1() as *const u8;
    let uid = frame.arg2() as u32;
    let gid = frame.arg3() as u32;

    let path = unsafe { read_user_string(path_ptr, 256)? };
    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    crate::fs::chown(&full_path, uid, gid)?;
    Ok(0)
}

/// `sys_fchown` (SYS_FCHOWN = 93)
pub fn sys_fchown(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let uid = frame.arg2() as u32;
    let gid = frame.arg3() as u32;

    if fd < 0 {
        return Err(SyscallError::EBADF);
    }
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(fd)?;
    drop(proc);

    file.ops
        .chown(uid, gid)
        .or_else(|_| file.dentry.inode.ops.chown(uid, gid))?;
    Ok(0)
}

/// `sys_lchown` (SYS_LCHOWN = 94)
pub fn sys_lchown(frame: &mut SyscallFrame) -> SyscallResult {
    let path_ptr = frame.arg1() as *const u8;
    let uid = frame.arg2() as u32;
    let gid = frame.arg3() as u32;

    let path = unsafe { read_user_string(path_ptr, 256)? };
    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    let dentry = crate::fs::resolve_path_nofollow(&full_path)?;
    dentry.inode.ops.chown(uid, gid)?;
    Ok(0)
}

/// `sys_fchownat` (SYS_FCHOWNAT = 260)
pub fn sys_fchownat(frame: &mut SyscallFrame) -> SyscallResult {
    let dfd = frame.arg1() as i32;
    let path_ptr = frame.arg2() as *const u8;
    let uid = frame.arg3() as u32;
    let gid = frame.arg4() as u32;
    let flags = frame.arg5() as i32;

    let path = unsafe { read_user_string(path_ptr, 256)? };
    let full_path = resolve_at_path(dfd, &path)?;
    if (flags & crate::fs::AT_SYMLINK_NOFOLLOW) != 0 {
        let dentry = crate::fs::resolve_path_nofollow(&full_path)?;
        dentry.inode.ops.chown(uid, gid)?;
    } else {
        crate::fs::chown(&full_path, uid, gid)?;
    }
    Ok(0)
}

/// `sys_mknodat` (SYS_MKNODAT = 259)
pub fn sys_mknodat(frame: &mut SyscallFrame) -> SyscallResult {
    let dfd = frame.arg1() as i32;
    let path_ptr = frame.arg2() as *const u8;
    let _mode = frame.arg3() as u32;
    let _dev = frame.arg4() as u64;

    let path = unsafe { read_user_string(path_ptr, 256)? };
    let full_path = resolve_at_path(dfd, &path)?;
    let _ = crate::fs::create_file(&full_path)?;
    Ok(0)
}

/// `sys_utimensat` (SYS_UTIMENSAT = 280)
pub fn sys_utimensat(frame: &mut SyscallFrame) -> SyscallResult {
    let dfd = frame.arg1() as i32;
    let path_ptr = frame.arg2() as *const u8;
    let times_ptr = frame.arg3() as *const LinuxTimespec;
    let flags = frame.arg4() as i32;

    let (atime, mtime) = if !times_ptr.is_null() {
        if !is_user_ptr_valid(times_ptr as u64, 2 * core::mem::size_of::<LinuxTimespec>()) {
            return Err(SyscallError::EFAULT);
        }
        let times = unsafe { core::slice::from_raw_parts(times_ptr, 2) };
        (times[0].tv_sec as u64, times[1].tv_sec as u64)
    } else {
        (0, 0)
    };

    if path_ptr.is_null() || (path_ptr as u64 == 0) {
        if dfd < 0 {
            return Err(SyscallError::EBADF);
        }
        let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
        let proc = proc_arc.lock();
        let file = proc.fd_table.get(dfd)?;
        drop(proc);
        file.ops
            .utimens(atime, mtime)
            .or_else(|_| file.dentry.inode.ops.utimens(atime, mtime))?;
        return Ok(0);
    }

    let path = unsafe { read_user_string(path_ptr, 256)? };
    if path.is_empty() && (flags & crate::fs::AT_EMPTY_PATH) != 0 {
        if dfd < 0 {
            return Err(SyscallError::EBADF);
        }
        let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
        let proc = proc_arc.lock();
        let file = proc.fd_table.get(dfd)?;
        drop(proc);
        file.ops
            .utimens(atime, mtime)
            .or_else(|_| file.dentry.inode.ops.utimens(atime, mtime))?;
        return Ok(0);
    }

    let full_path = resolve_at_path(dfd, &path)?;
    crate::fs::utimens(&full_path, atime, mtime)?;
    Ok(0)
}

/// `sys_futimesat` (SYS_FUTIMESAT = 261)
pub fn sys_futimesat(frame: &mut SyscallFrame) -> SyscallResult {
    let dfd = frame.arg1() as i32;
    let path_ptr = frame.arg2() as *const u8;
    let utimes_ptr = frame.arg3() as *const LinuxTimespec;

    if path_ptr.is_null() {
        return Ok(0);
    }
    let path = unsafe { read_user_string(path_ptr, 256)? };
    let full_path = resolve_at_path(dfd, &path)?;
    let (atime, mtime) = if !utimes_ptr.is_null() {
        if !is_user_ptr_valid(utimes_ptr as u64, 2 * core::mem::size_of::<LinuxTimespec>()) {
            return Err(SyscallError::EFAULT);
        }
        let times = unsafe { core::slice::from_raw_parts(utimes_ptr, 2) };
        (times[0].tv_sec as u64, times[1].tv_sec as u64)
    } else {
        (0, 0)
    };
    crate::fs::utimens(&full_path, atime, mtime)?;
    Ok(0)
}

/// `sys_faccessat` (SYS_FACCESSAT = 269)
/// Check user's permissions for a file relative to a directory file descriptor.
pub fn sys_faccessat(frame: &mut SyscallFrame) -> SyscallResult {
    let dfd = frame.arg1() as i32;
    let path_ptr = frame.arg2() as *const u8;
    let mode = frame.arg3() as i32;
    let _flags = frame.arg4() as i32;

    if mode < 0 || mode > 7 {
        return Err(SyscallError::EINVAL);
    }

    let path = unsafe { read_user_string(path_ptr, 256)? };
    let full_path = resolve_at_path(dfd, &path)?;
    let _dentry = crate::fs::resolve_path(&full_path)?;

    Ok(0)
}

fn copy_to_linux_stat(stat: &Stat) -> LinuxStat {
    LinuxStat {
        st_dev: 1,
        st_ino: stat.ino,
        st_nlink: stat.nlink as u64,
        st_mode: stat.mode,
        st_uid: stat.uid,
        st_gid: stat.gid,
        __pad0: 0,
        st_rdev: 0,
        st_size: stat.size as i64,
        st_blksize: if stat.blksize > 0 {
            stat.blksize as i64
        } else {
            4096
        },
        st_blocks: stat.blocks as i64,
        st_atime: stat.atime,
        st_atime_nsec: 0,
        st_mtime: stat.mtime,
        st_mtime_nsec: 0,
        st_ctime: stat.ctime,
        st_ctime_nsec: 0,
        __glibc_reserved: [0; 3],
    }
}

/// `sys_getdents64` (SYS_GETDENTS64 = 217)
/// Get directory entries in 64-bit Linux dirent format.
pub fn sys_getdents64(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let dirp = frame.arg2() as *mut u8;
    let count = frame.arg3() as usize;

    if fd < 0 {
        return Err(SyscallError::EBADF);
    }
    if count == 0 {
        return Ok(0);
    }
    if !is_user_ptr_valid(dirp as u64, count) {
        return Err(SyscallError::EFAULT);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(fd)?;
    drop(proc);

    if file.dentry.inode.inode_type != InodeType::Directory {
        return Err(SyscallError::ENOTDIR);
    }

    // Retrieve child entry names from directory inode
    let entries = file.dentry.inode.ops.readdir()?;

    // Construct full entry list with "." and ".." if not already explicitly returned
    let mut all_entries: alloc::vec::Vec<(String, u64, u8)> = alloc::vec::Vec::new();

    let has_dot = entries.iter().any(|e| e == ".");
    let has_dotdot = entries.iter().any(|e| e == "..");

    if !has_dot {
        all_entries.push((".".into(), file.dentry.inode.ino, DT_DIR));
    }
    if !has_dotdot {
        all_entries.push(("..".into(), file.dentry.inode.ino, DT_DIR));
    }

    for name in entries {
        if name == "." {
            all_entries.push((name, file.dentry.inode.ino, DT_DIR));
        } else if name == ".." {
            all_entries.push((name, file.dentry.inode.ino, DT_DIR));
        } else {
            let (child_ino, child_type) = match file.dentry.inode.ops.lookup(&name) {
                Ok(child) => {
                    let d_type = match child.inode_type {
                        InodeType::Directory => DT_DIR,
                        InodeType::File => DT_REG,
                        InodeType::CharDevice => DT_CHR,
                        InodeType::BlockDevice => DT_BLK,
                        InodeType::Symlink => DT_LNK,
                    };
                    (child.ino, d_type)
                }
                Err(_) => (1, DT_UNKNOWN),
            };
            all_entries.push((name, child_ino, child_type));
        }
    }

    let mut pos = *file.offset.lock();
    if pos >= all_entries.len() {
        return Ok(0);
    }

    let mut written_bytes = 0;

    while pos < all_entries.len() {
        let (ref name, ino, d_type) = all_entries[pos];
        let name_bytes = name.as_bytes();
        let unaligned_len = 19 + name_bytes.len() + 1;
        let reclen = (unaligned_len + 7) & !7;

        if written_bytes + reclen > count {
            if written_bytes == 0 {
                return Err(SyscallError::EINVAL);
            }
            break;
        }

        let off = (pos + 1) as i64;

        // SAFETY: Pointer and total range verified by is_user_ptr_valid above.
        unsafe {
            let dest = dirp.add(written_bytes);
            // Write d_ino (u64 at offset 0)
            core::ptr::write_volatile(dest as *mut u64, ino);
            // Write d_off (i64 at offset 8)
            core::ptr::write_volatile(dest.add(8) as *mut i64, off);
            // Write d_reclen (u16 at offset 16)
            core::ptr::write_volatile(dest.add(16) as *mut u16, reclen as u16);
            // Write d_type (u8 at offset 18)
            core::ptr::write(dest.add(18), d_type);
            // Write d_name (null-terminated string at offset 19)
            core::ptr::copy_nonoverlapping(name_bytes.as_ptr(), dest.add(19), name_bytes.len());
            // Null terminator
            core::ptr::write(dest.add(19 + name_bytes.len()), 0);
            // Zero padding up to reclen
            let pad_start = 19 + name_bytes.len() + 1;
            if pad_start < reclen {
                core::ptr::write_bytes(dest.add(pad_start), 0, reclen - pad_start);
            }
        }

        written_bytes += reclen;
        pos += 1;
    }

    *file.offset.lock() = pos;
    Ok(written_bytes)
}
