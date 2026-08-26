use super::{SyscallError, SyscallResult, UserCStr, UserPtr};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::File;
use crate::fs::vfs::types::{InodeType, LinuxStat, SeekWhence, Stat, StatFs};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

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

pub const POLLIN: i16 = 0x0001;
pub const POLLPRI: i16 = 0x0002;
pub const POLLOUT: i16 = 0x0004;
pub const POLLERR: i16 = 0x0008;
pub const POLLHUP: i16 = 0x0010;
pub const POLLNVAL: i16 = 0x0020;

/// `utimensat` special `tv_nsec` value: set the timestamp to the current time.
const UTIME_NOW: i64 = 0x3FFF_FFFF;
/// `utimensat` special `tv_nsec` value: leave the corresponding timestamp unchanged.
const UTIME_OMIT: i64 = 0x3FFF_FFFE;
/// Maximum valid nanosecond value within a timespec.
const NSEC_MAX: i64 = 999_999_999;

/// `sys_read` (SYS_READ = 0)
/// Read from a file descriptor.
pub fn sys_read(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let buf = UserPtr::<u8>::from_u64(frame.arg2());
    let count = frame.arg3() as usize;

    if fd < 0 {
        return Err(SyscallError::EBADF);
    }
    if count == 0 {
        return Ok(0);
    }
    let user_slice = buf.as_slice_mut(count).ok_or(SyscallError::EFAULT)?;

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();

    let file = proc.fd_table.get(fd)?;
    drop(proc);

    let bytes_read = file.read(user_slice)?;
    Ok(bytes_read)
}

/// `sys_write` (SYS_WRITE = 1)
/// Write to a file descriptor.
pub fn sys_write(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let buf = UserPtr::<u8>::from_u64(frame.arg2());
    let count = frame.arg3() as usize;

    if fd < 0 {
        return Err(SyscallError::EBADF);
    }
    if count == 0 {
        return Ok(0);
    }
    let user_slice = buf.as_slice(count).ok_or(SyscallError::EFAULT)?;

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();

    let file = proc.fd_table.get(fd)?;
    drop(proc);

    let bytes_written = file.write(user_slice)?;
    Ok(bytes_written)
}

/// `sys_pread64` (SYS_PREAD64 = 17)
/// Read from a file descriptor at a specified offset without changing the file position.
pub fn sys_pread64(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let buf = UserPtr::<u8>::from_u64(frame.arg2());
    let count = frame.arg3() as usize;
    let offset = frame.arg4() as usize;

    if fd < 0 {
        return Err(SyscallError::EBADF);
    }
    if count == 0 {
        return Ok(0);
    }
    let user_slice = buf.as_slice_mut(count).ok_or(SyscallError::EFAULT)?;

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();

    let file = proc.fd_table.get(fd)?;
    drop(proc);

    let bytes_read = file.pread(user_slice, offset)?;
    Ok(bytes_read)
}

/// `sys_pwrite64` (SYS_PWRITE64 = 18)
/// Write to a file descriptor at a specified offset without changing the file position.
pub fn sys_pwrite64(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let buf = UserPtr::<u8>::from_u64(frame.arg2());
    let count = frame.arg3() as usize;
    let offset = frame.arg4() as usize;

    if fd < 0 {
        return Err(SyscallError::EBADF);
    }
    if count == 0 {
        return Ok(0);
    }
    let user_slice = buf.as_slice(count).ok_or(SyscallError::EFAULT)?;

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();

    let file = proc.fd_table.get(fd)?;
    drop(proc);

    let bytes_written = file.pwrite(user_slice, offset)?;
    Ok(bytes_written)
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct IoVec {
    pub iov_base: u64,
    pub iov_len: usize,
}

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

/// Current wall-clock time in seconds since the UNIX epoch.
fn wall_now_secs() -> u64 {
    crate::drivers::time::cmos_rtc::get_wall_time().0
}

/// Resolve one user-provided timespec against the value currently stored on disk.
fn resolve_timespec(ts: LinuxTimespec, current: u64) -> Result<u64, SyscallError> {
    match ts.tv_nsec {
        UTIME_NOW => Ok(wall_now_secs()),
        UTIME_OMIT => Ok(current),
        n if (0..=NSEC_MAX).contains(&n) => Ok(ts.tv_sec as u64),
        _ => Err(SyscallError::EINVAL),
    }
}

/// Convert a user-provided timespec pair into concrete `(atime, mtime)`
/// seconds for `utimensat`. A null pointer selects "current time" for both
/// fields; `UTIME_OMIT` fields fall back to the current on-disk values.
fn read_utimens(
    times_ptr: UserPtr<LinuxTimespec>,
    cur_atime: u64,
    cur_mtime: u64,
) -> Result<(u64, u64), SyscallError> {
    if times_ptr.is_null() {
        let now = wall_now_secs();
        return Ok((now, now));
    }
    let times = times_ptr.as_slice(2).ok_or(SyscallError::EFAULT)?;
    Ok((
        resolve_timespec(times[0], cur_atime)?,
        resolve_timespec(times[1], cur_mtime)?,
    ))
}

/// Substitute the `(uid_t)-1` / `(gid_t)-1` "leave unchanged" sentinels with
/// the ownership currently recorded in `st`.
fn effective_owner(st: &Stat, uid: u32, gid: u32) -> (u32, u32) {
    (
        if uid == u32::MAX { st.uid } else { uid },
        if gid == u32::MAX { st.gid } else { gid },
    )
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
    let path_ptr = UserCStr::from_u64(frame.arg1());
    let flags = frame.arg2() as u32;

    let path = path_ptr.to_string(256)?;
    do_openat(AT_FDCWD, &path, flags)
}

/// `sys_openat` (SYS_OPENAT = 257)
/// Open a file relative to directory descriptor.
pub fn sys_openat(frame: &mut SyscallFrame) -> SyscallResult {
    let dfd = frame.arg1() as i32;
    let path_ptr = UserCStr::from_u64(frame.arg2());
    let flags = frame.arg3() as u32;

    let path = path_ptr.to_string(256)?;
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
    let path_ptr = UserCStr::from_u64(frame.arg1());
    let statbuf = UserPtr::<LinuxStat>::from_u64(frame.arg2());

    let path = path_ptr.to_string(256)?;
    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    let vfs_stat = crate::fs::stat(&full_path)?;

    let linux_stat = copy_to_linux_stat(&vfs_stat);
    statbuf.write(linux_stat).ok_or(SyscallError::EFAULT)?;

    Ok(0)
}

/// `sys_fstat` (SYS_FSTAT = 5)
/// Get file status by descriptor.
pub fn sys_fstat(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let statbuf = UserPtr::<LinuxStat>::from_u64(frame.arg2());

    if fd < 0 {
        return Err(SyscallError::EBADF);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(fd)?;
    drop(proc);

    let vfs_stat = file.ops.stat().or_else(|_| file.dentry.inode.ops.stat())?;
    let linux_stat = copy_to_linux_stat(&vfs_stat);

    statbuf.write(linux_stat).ok_or(SyscallError::EFAULT)?;

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
    let pipefd = UserPtr::<i32>::from_u64(frame.arg1());

    let (f_read, f_write) = crate::fs::pipefs::create_pipe(false)?;

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();

    let r_fd = proc.fd_table.alloc(f_read);
    let w_fd = proc.fd_table.alloc(f_write);

    pipefd.write(r_fd).ok_or(SyscallError::EFAULT)?;
    pipefd.offset(1).write(w_fd).ok_or(SyscallError::EFAULT)?;

    Ok(0)
}

/// `sys_pipe2` (SYS_PIPE2 = 293)
/// Create an anonymous pipe with flags.
pub fn sys_pipe2(frame: &mut SyscallFrame) -> SyscallResult {
    let pipefd = UserPtr::<i32>::from_u64(frame.arg1());
    let flags = frame.arg2() as u32;

    let nonblocking = (flags & O_NONBLOCK) != 0;
    let cloexec = if (flags & O_CLOEXEC) != 0 {
        crate::fs::fd::FD_CLOEXEC
    } else {
        0
    };

    let (f_read, f_write) = crate::fs::pipefs::create_pipe(nonblocking)?;

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();

    let r_fd = proc.fd_table.alloc_with_flags(f_read, cloexec);
    let w_fd = proc.fd_table.alloc_with_flags(f_write, cloexec);

    pipefd.write(r_fd).ok_or(SyscallError::EFAULT)?;
    pipefd.offset(1).write(w_fd).ok_or(SyscallError::EFAULT)?;

    Ok(0)
}

/// `sys_getcwd` (SYS_GETCWD = 79)
/// Get current working directory string.
pub fn sys_getcwd(frame: &mut SyscallFrame) -> SyscallResult {
    let buf = UserPtr::<u8>::from_u64(frame.arg1());
    let size = frame.arg2() as usize;

    if size == 0 || !buf.is_valid_for(size) {
        return Err(SyscallError::EINVAL);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();

    let cwd_bytes = proc.cwd.as_bytes();
    if cwd_bytes.len() + 1 > size {
        return Err(SyscallError::ENOMEM);
    }

    buf.write_slice(cwd_bytes).ok_or(SyscallError::EFAULT)?;
    buf.offset(cwd_bytes.len())
        .write(0)
        .ok_or(SyscallError::EFAULT)?;

    Ok(buf.as_u64() as usize)
}

/// `sys_chdir` (SYS_CHDIR = 80)
/// Change working directory.
pub fn sys_chdir(frame: &mut SyscallFrame) -> SyscallResult {
    let path = UserCStr::from_u64(frame.arg1()).to_string(256)?;

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
            Ok(file.flags() as usize)
        }
        F_SETFL => {
            let flags = arg as u32;
            let file = proc.fd_table.get(fd)?;
            file.set_flags(flags);
            Ok(0)
        }
        F_GETLK | F_SETLK | F_SETLKW | F_SETOWN | F_GETOWN => Ok(0),
        _ => Ok(0),
    }
}

/// `sys_flock` (SYS_FLOCK = 73)
/// Apply or remove an advisory lock on an open file.
pub fn sys_flock(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let _operation = frame.arg2() as i32;

    if fd < 0 {
        return Err(SyscallError::EBADF);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let _file = proc.fd_table.get(fd)?;
    drop(proc);

    // Advisory file locking
    Ok(0)
}

/// `sys_fchdir` (SYS_FCHDIR = 81)
/// Change working directory using an open directory file descriptor.
pub fn sys_fchdir(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    if fd < 0 {
        return Err(SyscallError::EBADF);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let mut proc = proc_arc.lock();

    let file = proc.fd_table.get(fd)?;
    if file.dentry.inode.inode_type != InodeType::Directory {
        return Err(SyscallError::ENOTDIR);
    }

    proc.cwd = file.dentry.full_path();
    Ok(0)
}

const RAMFS_MAGIC: i64 = 0x858458f6;

fn make_statfs() -> StatFs {
    StatFs {
        f_type: RAMFS_MAGIC,
        f_bsize: 4096,
        f_blocks: 262144, // ~1GB
        f_bfree: 200000,
        f_bavail: 200000,
        f_files: 65536,
        f_ffree: 60000,
        f_fsid: [0, 0],
        f_namelen: 255,
        f_frsize: 4096,
        f_flags: 0,
        f_spare: [0; 4],
    }
}

/// `sys_statfs` (SYS_STATFS = 137)
/// Get filesystem statistics by pathname.
pub fn sys_statfs(frame: &mut SyscallFrame) -> SyscallResult {
    let path = UserCStr::from_u64(frame.arg1()).to_string(256)?;
    let buf_ptr = UserPtr::<StatFs>::from_u64(frame.arg2());

    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    let _dentry = crate::fs::resolve_path(&full_path)?;

    let statfs = make_statfs();
    buf_ptr.write(statfs).ok_or(SyscallError::EFAULT)?;
    Ok(0)
}

/// `sys_fstatfs` (SYS_FSTATFS = 138)
/// Get filesystem statistics by open file descriptor.
pub fn sys_fstatfs(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let buf_ptr = UserPtr::<StatFs>::from_u64(frame.arg2());

    if fd < 0 {
        return Err(SyscallError::EBADF);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let _file = proc.fd_table.get(fd)?;
    drop(proc);

    let statfs = make_statfs();
    buf_ptr.write(statfs).ok_or(SyscallError::EFAULT)?;
    Ok(0)
}

/// `sys_access` (SYS_ACCESS = 21)
/// Check user's permissions for a file.
pub fn sys_access(frame: &mut SyscallFrame) -> SyscallResult {
    let mode = frame.arg2() as i32;

    if mode < 0 || mode > 7 {
        return Err(SyscallError::EINVAL);
    }

    let path = UserCStr::from_u64(frame.arg1()).to_string(256)?;
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
    let path_ptr = UserCStr::from_u64(frame.arg2());
    let statbuf = UserPtr::<LinuxStat>::from_u64(frame.arg3());

    let path = path_ptr.to_string(256)?;
    let full_path = resolve_at_path(dfd, &path)?;
    let vfs_stat = crate::fs::stat(&full_path)?;

    let linux_stat = copy_to_linux_stat(&vfs_stat);
    statbuf.write(linux_stat).ok_or(SyscallError::EFAULT)?;

    Ok(0)
}

/// `sys_lstat` (SYS_LSTAT = 6)
/// Get file status without following symlinks.
pub fn sys_lstat(frame: &mut SyscallFrame) -> SyscallResult {
    let path_ptr = UserCStr::from_u64(frame.arg1());
    let statbuf = UserPtr::<LinuxStat>::from_u64(frame.arg2());

    let path = path_ptr.to_string(256)?;
    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    let vfs_stat = crate::fs::lstat(&full_path)?;

    let linux_stat = copy_to_linux_stat(&vfs_stat);
    statbuf.write(linux_stat).ok_or(SyscallError::EFAULT)?;

    Ok(0)
}

/// `sys_poll` (SYS_POLL = 7)
pub fn sys_poll(frame: &mut SyscallFrame) -> SyscallResult {
    let fds_ptr = UserPtr::<PollFd>::from_u64(frame.arg1());
    let nfds = frame.arg2() as usize;
    let timeout_ms = frame.arg3() as i32;

    do_poll(fds_ptr, nfds, timeout_ms)
}

/// `sys_ppoll` (SYS_PPOLL = 271)
pub fn sys_ppoll(frame: &mut SyscallFrame) -> SyscallResult {
    let fds_ptr = UserPtr::<PollFd>::from_u64(frame.arg1());
    let nfds = frame.arg2() as usize;
    let ts_ptr = UserPtr::<crate::syscalls::time::TimeSpec>::from_u64(frame.arg3());

    let timeout_ms = if ts_ptr.is_null() {
        -1
    } else {
        let ts = ts_ptr.read().ok_or(SyscallError::EFAULT)?;
        if ts.tv_sec < 0 || ts.tv_nsec < 0 {
            return Err(SyscallError::EINVAL);
        }
        (ts.tv_sec * 1000 + ts.tv_nsec / 1_000_000) as i32
    };

    do_poll(fds_ptr, nfds, timeout_ms)
}

fn do_poll(fds_ptr: UserPtr<PollFd>, nfds: usize, timeout_ms: i32) -> SyscallResult {
    if nfds == 0 {
        if timeout_ms > 0 {
            let start_ns = crate::arch::timer::hpet::elapsed_ns();
            let dur_ns = (timeout_ms as u64) * 1_000_000;
            while crate::arch::timer::hpet::elapsed_ns().saturating_sub(start_ns) < dur_ns {
                crate::arch::enable_interrupts();
                crate::proc::thread::Thread::yield_cpu();
            }
        }
        return Ok(0);
    }
    if nfds > 1024 {
        return Err(SyscallError::EFAULT);
    }
    let fds_slice = fds_ptr.as_slice_mut(nfds).ok_or(SyscallError::EFAULT)?;

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;

    let start_ns = crate::arch::timer::hpet::elapsed_ns();
    let has_timeout = timeout_ms >= 0;
    let dur_ns = if timeout_ms > 0 {
        (timeout_ms as u64) * 1_000_000
    } else {
        0
    };

    loop {
        // Retrieve file handles under proc lock, then drop proc lock before polling events
        let files: Vec<Option<Arc<File>>> = {
            let proc = proc_arc.lock();
            fds_slice
                .iter()
                .map(|pfd| {
                    if pfd.fd >= 0 {
                        proc.fd_table.get(pfd.fd).ok()
                    } else {
                        None
                    }
                })
                .collect()
        };

        let mut ready_count = 0;
        for (pfd, file_opt) in fds_slice.iter_mut().zip(files.iter()) {
            if pfd.fd < 0 {
                pfd.revents = 0;
                continue;
            }
            match file_opt {
                Some(file) => {
                    let revents = file.ops.poll_events(pfd.events);
                    pfd.revents = revents;
                    if revents != 0 {
                        ready_count += 1;
                    }
                }
                None => {
                    pfd.revents = POLLNVAL;
                    ready_count += 1;
                }
            }
        }

        if ready_count > 0 {
            return Ok(ready_count);
        }

        if has_timeout {
            if timeout_ms == 0 {
                return Ok(0);
            }
            if crate::arch::timer::hpet::elapsed_ns().saturating_sub(start_ns) >= dur_ns {
                return Ok(0);
            }
        }

        // Check if there are pending signals interrupting poll
        {
            let proc = proc_arc.lock();
            if proc.pending_signals.mask != 0 {
                return Err(SyscallError::EINTR);
            }
        }

        crate::arch::enable_and_hlt();
    }
}

/// `sys_readv` (SYS_READV = 19)
pub fn sys_readv(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let iov_ptr = UserPtr::<IoVec>::from_u64(frame.arg2());
    let iovcnt = frame.arg3() as usize;

    if fd < 0 {
        return Err(SyscallError::EBADF);
    }
    if iovcnt == 0 {
        return Ok(0);
    }
    if iovcnt > 1024 {
        return Err(SyscallError::EFAULT);
    }
    let iov_slice = iov_ptr.as_slice(iovcnt).ok_or(SyscallError::EFAULT)?;

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(fd)?;
    drop(proc);

    let mut total_read = 0;
    for iov in iov_slice {
        if iov.iov_len == 0 {
            continue;
        }
        let base_ptr = UserPtr::<u8>::from_u64(iov.iov_base);
        let user_slice = base_ptr
            .as_slice_mut(iov.iov_len)
            .ok_or(SyscallError::EFAULT)?;
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
    let iov_ptr = UserPtr::<IoVec>::from_u64(frame.arg2());
    let iovcnt = frame.arg3() as usize;

    if fd < 0 {
        return Err(SyscallError::EBADF);
    }
    if iovcnt == 0 {
        return Ok(0);
    }
    if iovcnt > 1024 {
        return Err(SyscallError::EFAULT);
    }
    let iov_slice = iov_ptr.as_slice(iovcnt).ok_or(SyscallError::EFAULT)?;

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(fd)?;
    drop(proc);

    let mut total_written = 0;
    for iov in iov_slice {
        if iov.iov_len == 0 {
            continue;
        }
        let base_ptr = UserPtr::<u8>::from_u64(iov.iov_base);
        let user_slice = base_ptr.as_slice(iov.iov_len).ok_or(SyscallError::EFAULT)?;
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
    let readfds = UserPtr::<FdSet>::from_u64(frame.arg2());
    let writefds = UserPtr::<FdSet>::from_u64(frame.arg3());
    let exceptfds = UserPtr::<FdSet>::from_u64(frame.arg4());
    let _timeout = UserPtr::<LinuxTimespec>::from_u64(frame.arg5());

    if nfds < 0 || nfds > FD_SETSIZE as i32 {
        return Err(SyscallError::EINVAL);
    }
    if nfds == 0 {
        return Ok(0);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();

    let mut ready_count = 0;
    let mut rfds_val = if !readfds.is_null() {
        Some(readfds.read().ok_or(SyscallError::EFAULT)?)
    } else {
        None
    };
    let mut wfds_val = if !writefds.is_null() {
        Some(writefds.read().ok_or(SyscallError::EFAULT)?)
    } else {
        None
    };
    let mut efds_val = if !exceptfds.is_null() {
        Some(exceptfds.read().ok_or(SyscallError::EFAULT)?)
    } else {
        None
    };

    for fd in 0..nfds {
        let word = (fd / 64) as usize;
        let bit = 1u64 << (fd % 64);

        let mut is_r = false;
        let mut is_w = false;

        if let Some(ref r) = rfds_val {
            if (r.fds_bits[word] & bit) != 0 {
                is_r = true;
            }
        }
        if let Some(ref w) = wfds_val {
            if (w.fds_bits[word] & bit) != 0 {
                is_w = true;
            }
        }
        if let Some(ref mut e) = efds_val {
            e.fds_bits[word] &= !bit;
        }

        if is_r || is_w {
            if let Ok(file) = proc.fd_table.get(fd) {
                let flags = file.flags();
                if is_r && crate::fs::can_read(flags) {
                    ready_count += 1;
                } else if let Some(ref mut r) = rfds_val {
                    r.fds_bits[word] &= !bit;
                }
                if is_w && crate::fs::can_write(flags) {
                    ready_count += 1;
                } else if let Some(ref mut w) = wfds_val {
                    w.fds_bits[word] &= !bit;
                }
            } else {
                if let Some(ref mut r) = rfds_val {
                    r.fds_bits[word] &= !bit;
                }
                if let Some(ref mut w) = wfds_val {
                    w.fds_bits[word] &= !bit;
                }
            }
        }
    }

    if let Some(r) = rfds_val {
        readfds.write(r).ok_or(SyscallError::EFAULT)?;
    }
    if let Some(w) = wfds_val {
        writefds.write(w).ok_or(SyscallError::EFAULT)?;
    }
    if let Some(e) = efds_val {
        exceptfds.write(e).ok_or(SyscallError::EFAULT)?;
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
    let length = frame.arg2() as usize;

    let path = UserCStr::from_u64(frame.arg1()).to_string(256)?;
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
    let old_path = UserCStr::from_u64(frame.arg1()).to_string(256)?;
    let new_path = UserCStr::from_u64(frame.arg2()).to_string(256)?;
    let old_full = resolve_at_path(AT_FDCWD, &old_path)?;
    let new_full = resolve_at_path(AT_FDCWD, &new_path)?;
    crate::fs::rename(&old_full, &new_full)?;
    Ok(0)
}

/// `sys_renameat` (SYS_RENAMEAT = 264)
pub fn sys_renameat(frame: &mut SyscallFrame) -> SyscallResult {
    let olddfd = frame.arg1() as i32;
    let newdfd = frame.arg3() as i32;

    let old_path = UserCStr::from_u64(frame.arg2()).to_string(256)?;
    let new_path = UserCStr::from_u64(frame.arg4()).to_string(256)?;
    let old_full = resolve_at_path(olddfd, &old_path)?;
    let new_full = resolve_at_path(newdfd, &new_path)?;
    crate::fs::rename(&old_full, &new_full)?;
    Ok(0)
}

/// `sys_renameat2` (SYS_RENAMEAT2 = 316)
pub fn sys_renameat2(frame: &mut SyscallFrame) -> SyscallResult {
    let olddfd = frame.arg1() as i32;
    let newdfd = frame.arg3() as i32;
    let _flags = frame.arg5() as u32;

    let old_path = UserCStr::from_u64(frame.arg2()).to_string(256)?;
    let new_path = UserCStr::from_u64(frame.arg4()).to_string(256)?;
    let old_full = resolve_at_path(olddfd, &old_path)?;
    let new_full = resolve_at_path(newdfd, &new_path)?;
    crate::fs::rename(&old_full, &new_full)?;
    Ok(0)
}

/// `sys_mkdir` (SYS_MKDIR = 83)
pub fn sys_mkdir(frame: &mut SyscallFrame) -> SyscallResult {
    let _mode = frame.arg2() as u32;

    let path = UserCStr::from_u64(frame.arg1()).to_string(256)?;
    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    crate::fs::mkdir(&full_path)?;
    Ok(0)
}

/// `sys_mkdirat` (SYS_MKDIRAT = 258)
pub fn sys_mkdirat(frame: &mut SyscallFrame) -> SyscallResult {
    let dfd = frame.arg1() as i32;
    let _mode = frame.arg3() as u32;

    let path = UserCStr::from_u64(frame.arg2()).to_string(256)?;
    let full_path = resolve_at_path(dfd, &path)?;
    crate::fs::mkdir(&full_path)?;
    Ok(0)
}

/// `sys_rmdir` (SYS_RMDIR = 84)
pub fn sys_rmdir(frame: &mut SyscallFrame) -> SyscallResult {
    let path = UserCStr::from_u64(frame.arg1()).to_string(256)?;
    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    crate::fs::rmdir(&full_path)?;
    Ok(0)
}

/// `sys_link` (SYS_LINK = 86)
pub fn sys_link(frame: &mut SyscallFrame) -> SyscallResult {
    let old_path = UserCStr::from_u64(frame.arg1()).to_string(256)?;
    let new_path = UserCStr::from_u64(frame.arg2()).to_string(256)?;
    let old_full = resolve_at_path(AT_FDCWD, &old_path)?;
    let new_full = resolve_at_path(AT_FDCWD, &new_path)?;
    crate::fs::link(&old_full, &new_full)?;
    Ok(0)
}

/// `sys_linkat` (SYS_LINKAT = 265)
pub fn sys_linkat(frame: &mut SyscallFrame) -> SyscallResult {
    let olddfd = frame.arg1() as i32;
    let newdfd = frame.arg3() as i32;
    let _flags = frame.arg5() as i32;

    let old_path = UserCStr::from_u64(frame.arg2()).to_string(256)?;
    let new_path = UserCStr::from_u64(frame.arg4()).to_string(256)?;
    let old_full = resolve_at_path(olddfd, &old_path)?;
    let new_full = resolve_at_path(newdfd, &new_path)?;
    crate::fs::link(&old_full, &new_full)?;
    Ok(0)
}

/// `sys_unlink` (SYS_UNLINK = 87)
pub fn sys_unlink(frame: &mut SyscallFrame) -> SyscallResult {
    let path = UserCStr::from_u64(frame.arg1()).to_string(256)?;
    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    crate::fs::unlink(&full_path)?;
    Ok(0)
}

/// `sys_unlinkat` (SYS_UNLINKAT = 263)
pub fn sys_unlinkat(frame: &mut SyscallFrame) -> SyscallResult {
    let dfd = frame.arg1() as i32;
    let flags = frame.arg3() as i32;

    let path = UserCStr::from_u64(frame.arg2()).to_string(256)?;
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
    let target = UserCStr::from_u64(frame.arg1()).to_string(256)?;
    let link_path = UserCStr::from_u64(frame.arg2()).to_string(256)?;
    let full_path = resolve_at_path(AT_FDCWD, &link_path)?;
    crate::fs::symlink(&full_path, &target)?;
    Ok(0)
}

/// `sys_symlinkat` (SYS_SYMLINKAT = 266)
pub fn sys_symlinkat(frame: &mut SyscallFrame) -> SyscallResult {
    let newdfd = frame.arg2() as i32;

    let target = UserCStr::from_u64(frame.arg1()).to_string(256)?;
    let link_path = UserCStr::from_u64(frame.arg3()).to_string(256)?;
    let full_path = resolve_at_path(newdfd, &link_path)?;
    crate::fs::symlink(&full_path, &target)?;
    Ok(0)
}

/// `sys_readlink` (SYS_READLINK = 89)
pub fn sys_readlink(frame: &mut SyscallFrame) -> SyscallResult {
    let path_ptr = UserCStr::from_u64(frame.arg1());
    let buf = UserPtr::<u8>::from_u64(frame.arg2());
    let bufsiz = frame.arg3() as usize;

    if bufsiz == 0 || !buf.is_valid_for(bufsiz) {
        return Err(SyscallError::EINVAL);
    }

    let path = path_ptr.to_string(256)?;
    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    let target = crate::fs::readlink(&full_path)?;
    let target_bytes = target.as_bytes();
    let copy_len = core::cmp::min(target_bytes.len(), bufsiz);

    let user_slice = buf.as_slice_mut(copy_len).ok_or(SyscallError::EFAULT)?;
    user_slice.copy_from_slice(&target_bytes[..copy_len]);

    Ok(copy_len)
}

/// `sys_readlinkat` (SYS_READLINKAT = 267)
pub fn sys_readlinkat(frame: &mut SyscallFrame) -> SyscallResult {
    let dfd = frame.arg1() as i32;
    let path_ptr = UserCStr::from_u64(frame.arg2());
    let buf = UserPtr::<u8>::from_u64(frame.arg3());
    let bufsiz = frame.arg4() as usize;

    if bufsiz == 0 || !buf.is_valid_for(bufsiz) {
        return Err(SyscallError::EINVAL);
    }

    let path = path_ptr.to_string(256)?;
    let full_path = resolve_at_path(dfd, &path)?;
    let target = crate::fs::readlink(&full_path)?;
    let target_bytes = target.as_bytes();
    let copy_len = core::cmp::min(target_bytes.len(), bufsiz);

    let user_slice = buf.as_slice_mut(copy_len).ok_or(SyscallError::EFAULT)?;
    user_slice.copy_from_slice(&target_bytes[..copy_len]);

    Ok(copy_len)
}

/// `sys_chmod` (SYS_CHMOD = 90)
pub fn sys_chmod(frame: &mut SyscallFrame) -> SyscallResult {
    let mode = frame.arg2() as u32;

    let path = UserCStr::from_u64(frame.arg1()).to_string(256)?;
    let full_path = resolve_at_path(AT_FDCWD, &path)?;

    let st = crate::fs::stat(&full_path)?;
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let creds = { Arc::clone(&proc_arc.lock().creds) };
    if creds.euid != 0 && creds.euid != st.uid {
        return Err(SyscallError::EPERM);
    }

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
    let creds = Arc::clone(&proc.creds);
    drop(proc);

    let st = file.ops.stat().or_else(|_| file.dentry.inode.ops.stat())?;
    if creds.euid != 0 && creds.euid != st.uid {
        return Err(SyscallError::EPERM);
    }

    file.ops
        .chmod(mode)
        .or_else(|_| file.dentry.inode.ops.chmod(mode))?;
    Ok(0)
}

/// `sys_fchmodat` (SYS_FCHMODAT = 268)
pub fn sys_fchmodat(frame: &mut SyscallFrame) -> SyscallResult {
    let dfd = frame.arg1() as i32;
    let mode = frame.arg3() as u32;
    let _flags = frame.arg4() as i32;

    let path = UserCStr::from_u64(frame.arg2()).to_string(256)?;
    let full_path = resolve_at_path(dfd, &path)?;
    crate::fs::chmod(&full_path, mode)?;
    Ok(0)
}

/// `sys_chown` (SYS_CHOWN = 92)
pub fn sys_chown(frame: &mut SyscallFrame) -> SyscallResult {
    let uid = frame.arg2() as u32;
    let gid = frame.arg3() as u32;

    let path = UserCStr::from_u64(frame.arg1()).to_string(256)?;
    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    let st = crate::fs::stat(&full_path)?;
    let (uid, gid) = effective_owner(&st, uid, gid);

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let creds = { Arc::clone(&proc_arc.lock().creds) };

    if creds.euid != 0 {
        if uid != st.uid || (gid != st.gid && gid != creds.gid && gid != creds.egid) {
            return Err(SyscallError::EPERM);
        }
    }

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
    let creds = Arc::clone(&proc.creds);
    drop(proc);

    let st = file.dentry.inode.ops.stat()?;
    let (uid, gid) = effective_owner(&st, uid, gid);

    if creds.euid != 0 {
        if uid != st.uid || (gid != st.gid && gid != creds.gid && gid != creds.egid) {
            return Err(SyscallError::EPERM);
        }
    }

    file.ops
        .chown(uid, gid)
        .or_else(|_| file.dentry.inode.ops.chown(uid, gid))?;
    Ok(0)
}

/// `sys_lchown` (SYS_LCHOWN = 94)
pub fn sys_lchown(frame: &mut SyscallFrame) -> SyscallResult {
    let uid = frame.arg2() as u32;
    let gid = frame.arg3() as u32;

    let path = UserCStr::from_u64(frame.arg1()).to_string(256)?;
    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    let dentry = crate::fs::resolve_path_nofollow(&full_path)?;
    let st = dentry.inode.ops.stat()?;
    let (uid, gid) = effective_owner(&st, uid, gid);
    dentry.inode.ops.chown(uid, gid)?;
    Ok(0)
}

/// `sys_fchownat` (SYS_FCHOWNAT = 260)
pub fn sys_fchownat(frame: &mut SyscallFrame) -> SyscallResult {
    let dfd = frame.arg1() as i32;
    let uid = frame.arg3() as u32;
    let gid = frame.arg4() as u32;
    let flags = frame.arg5() as i32;

    let path = UserCStr::from_u64(frame.arg2()).to_string(256)?;
    let full_path = resolve_at_path(dfd, &path)?;
    if (flags & crate::fs::AT_SYMLINK_NOFOLLOW) != 0 {
        let dentry = crate::fs::resolve_path_nofollow(&full_path)?;
        let st = dentry.inode.ops.stat()?;
        let (uid, gid) = effective_owner(&st, uid, gid);
        dentry.inode.ops.chown(uid, gid)?;
    } else {
        let st = crate::fs::stat(&full_path)?;
        let (uid, gid) = effective_owner(&st, uid, gid);
        crate::fs::chown(&full_path, uid, gid)?;
    }
    Ok(0)
}

/// `sys_mknodat` (SYS_MKNODAT = 259)
pub fn sys_mknodat(frame: &mut SyscallFrame) -> SyscallResult {
    let dfd = frame.arg1() as i32;
    let _mode = frame.arg3() as u32;
    let _dev = frame.arg4() as u64;

    let path = UserCStr::from_u64(frame.arg2()).to_string(256)?;
    let full_path = resolve_at_path(dfd, &path)?;
    let _ = crate::fs::create_file(&full_path)?;
    Ok(0)
}

/// `sys_utimensat` (SYS_UTIMENSAT = 280)
pub fn sys_utimensat(frame: &mut SyscallFrame) -> SyscallResult {
    let dfd = frame.arg1() as i32;
    let path_cstr = UserCStr::from_u64(frame.arg2());
    let times_ptr = UserPtr::<LinuxTimespec>::from_u64(frame.arg3());
    let flags = frame.arg4() as i32;

    if path_cstr.is_null() || (path_cstr.as_u64() == 0) {
        if dfd < 0 {
            return Err(SyscallError::EBADF);
        }
        let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
        let creds = { Arc::clone(&proc_arc.lock().creds) };
        let proc = proc_arc.lock();
        let file = proc.fd_table.get(dfd)?;
        drop(proc);
        let st = file.ops.stat().or_else(|_| file.dentry.inode.ops.stat())?;

        if creds.euid != 0 && creds.euid != st.uid {
            return Err(SyscallError::EPERM);
        }

        let (atime, mtime) = read_utimens(times_ptr, st.atime, st.mtime)?;
        file.ops
            .utimens(atime, mtime)
            .or_else(|_| file.dentry.inode.ops.utimens(atime, mtime))?;
        return Ok(0);
    }

    let path = path_cstr.to_string(256)?;
    if path.is_empty() && (flags & crate::fs::AT_EMPTY_PATH) != 0 {
        if dfd < 0 {
            return Err(SyscallError::EBADF);
        }
        let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
        let creds = { Arc::clone(&proc_arc.lock().creds) };
        let proc = proc_arc.lock();
        let file = proc.fd_table.get(dfd)?;
        drop(proc);
        let st = file.ops.stat().or_else(|_| file.dentry.inode.ops.stat())?;

        if creds.euid != 0 && creds.euid != st.uid {
            return Err(SyscallError::EPERM);
        }

        let (atime, mtime) = read_utimens(times_ptr, st.atime, st.mtime)?;
        file.ops
            .utimens(atime, mtime)
            .or_else(|_| file.dentry.inode.ops.utimens(atime, mtime))?;
        return Ok(0);
    }

    let full_path = resolve_at_path(dfd, &path)?;
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let creds = { Arc::clone(&proc_arc.lock().creds) };
    let st = crate::fs::stat(&full_path)?;

    if creds.euid != 0 && creds.euid != st.uid {
        return Err(SyscallError::EPERM);
    }

    let (atime, mtime) = read_utimens(times_ptr, st.atime, st.mtime)?;
    crate::fs::utimens(&full_path, atime, mtime)?;
    Ok(0)
}

/// `sys_futimesat` (SYS_FUTIMESAT = 261)
pub fn sys_futimesat(frame: &mut SyscallFrame) -> SyscallResult {
    let dfd = frame.arg1() as i32;
    let path_cstr = UserCStr::from_u64(frame.arg2());
    let utimes_ptr = UserPtr::<LinuxTimespec>::from_u64(frame.arg3());

    if path_cstr.is_null() {
        return Ok(0);
    }
    let path = path_cstr.to_string(256)?;
    let full_path = resolve_at_path(dfd, &path)?;
    let (atime, mtime) = if !utimes_ptr.is_null() {
        let times = utimes_ptr.as_slice(2).ok_or(SyscallError::EFAULT)?;
        (
            resolve_timespec(times[0], 0)?,
            resolve_timespec(times[1], 0)?,
        )
    } else {
        // POSIX: a null timeval selects the current time for both fields.
        let now = wall_now_secs();
        (now, now)
    };
    crate::fs::utimens(&full_path, atime, mtime)?;
    Ok(0)
}

/// `sys_faccessat` (SYS_FACCESSAT = 269)
/// Check user's permissions for a file relative to a directory file descriptor.
pub fn sys_faccessat(frame: &mut SyscallFrame) -> SyscallResult {
    let dfd = frame.arg1() as i32;
    let mode = frame.arg3() as i32;
    let _flags = frame.arg4() as i32;

    if mode < 0 || mode > 7 {
        return Err(SyscallError::EINVAL);
    }

    let path = UserCStr::from_u64(frame.arg2()).to_string(256)?;
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
    let dirp = UserPtr::<u8>::from_u64(frame.arg2());
    let count = frame.arg3() as usize;

    if fd < 0 {
        return Err(SyscallError::EBADF);
    }
    if count == 0 {
        return Ok(0);
    }
    if !dirp.is_valid_for(count) {
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
    let mut all_entries: Vec<(String, u64, u8)> = Vec::new();

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

        let dest = dirp.offset(written_bytes);
        dest.cast::<u64>().write(ino).ok_or(SyscallError::EFAULT)?;
        dest.offset(8)
            .cast::<i64>()
            .write(off)
            .ok_or(SyscallError::EFAULT)?;
        dest.offset(16)
            .cast::<u16>()
            .write(reclen as u16)
            .ok_or(SyscallError::EFAULT)?;
        dest.offset(18).write(d_type).ok_or(SyscallError::EFAULT)?;
        dest.offset(19)
            .write_slice(name_bytes)
            .ok_or(SyscallError::EFAULT)?;
        dest.offset(19 + name_bytes.len())
            .write(0)
            .ok_or(SyscallError::EFAULT)?;

        let pad_start = 19 + name_bytes.len() + 1;
        for p in pad_start..reclen {
            dest.offset(p).write(0).ok_or(SyscallError::EFAULT)?;
        }

        written_bytes += reclen;
        pos += 1;
    }

    *file.offset.lock() = pos;
    Ok(written_bytes)
}

/// `sys_fadvise64` (SYS_FADVISE64 = 221)
/// Predeclare an access pattern for file data.
pub fn sys_fadvise64(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let _offset = frame.arg2() as i64;
    let len = frame.arg3() as i64;
    let advice = frame.arg4() as i32;

    if fd < 0 {
        return Err(SyscallError::EBADF);
    }

    if len < 0 {
        return Err(SyscallError::EINVAL);
    }

    // POSIX advice values (POSIX_FADV_NORMAL=0, RANDOM=1, SEQUENTIAL=2, WILLNEED=3, DONTNEED=4, NOREUSE=5)
    if !(0..=5).contains(&advice) {
        return Err(SyscallError::EINVAL);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let _file = proc.fd_table.get(fd)?;
    drop(proc);

    // Advisory hint acknowledged
    Ok(0)
}
