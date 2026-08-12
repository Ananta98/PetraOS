use super::{is_user_ptr_valid, read_user_string, SyscallError, SyscallResult};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::vfs::types::{File, LinuxStat, O_CREAT, O_RDONLY, O_WRONLY, SeekWhence, Stat};
use alloc::sync::Arc;

pub const AT_FDCWD: i32 = -100;

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

    if fd == 1 || fd == 2 {
        if let Ok(s) = core::str::from_utf8(user_slice) {
            log::info!("[Userspace Output] {}", s.trim_end());
        }
        return Ok(count);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();

    let file = proc.fd_table.get(fd)?;
    drop(proc);

    let bytes_written = file.write(user_slice)?;
    Ok(bytes_written)
}

/// `sys_open` (SYS_OPEN = 2)
/// Open a file.
pub fn sys_open(frame: &mut SyscallFrame) -> SyscallResult {
    let path_ptr = frame.arg1() as *const u8;
    let flags = frame.arg2() as u32;

    let path = unsafe { read_user_string(path_ptr, 256)? };

    let dentry = match crate::fs::resolve_path(&path) {
        Ok(d) => d,
        Err(crate::fs::vfs::types::VfsError::NotFound) if (flags & O_CREAT) != 0 => {
            crate::fs::create_file(&path)?
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
    let vfs_stat = crate::fs::stat(&path)?;

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

    let new_offset = file.lseek(offset, whence)?;
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
    proc.fd_table.set(newfd, file)?;

    Ok(newfd as usize)
}

/// `sys_dup3` (SYS_DUP3 = 292)
/// Duplicate a file descriptor with flags.
pub fn sys_dup3(frame: &mut SyscallFrame) -> SyscallResult {
    let oldfd = frame.arg1() as i32;
    let newfd = frame.arg2() as i32;

    if oldfd == newfd {
        return Err(SyscallError::EINVAL);
    }
    sys_dup2(frame)
}

/// `sys_pipe` (SYS_PIPE = 22)
/// Create an anonymous inter-process pipe.
pub fn sys_pipe(frame: &mut SyscallFrame) -> SyscallResult {
    let pipefd = frame.arg1() as *mut i32;
    if !is_user_ptr_valid(pipefd as u64, 2 * core::mem::size_of::<i32>()) {
        return Err(SyscallError::EFAULT);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();

    if let Ok(dentry) = crate::fs::resolve_path("/") {
        if let Ok(file_ops) = dentry.inode.ops.open() {
            let f_read = Arc::new(File::new(dentry.clone(), O_RDONLY, file_ops.clone()));
            let f_write = Arc::new(File::new(dentry, O_WRONLY, file_ops));

            let r_fd = proc.fd_table.alloc(f_read);
            let w_fd = proc.fd_table.alloc(f_write);

            // SAFETY: User pipefd pointer range validated within Ring 3 address bounds.
            unsafe {
                core::ptr::write_volatile(pipefd, r_fd);
                core::ptr::write_volatile(pipefd.add(1), w_fd);
            }
            return Ok(0);
        }
    }

    Err(SyscallError::EMFILE)
}

/// `sys_pipe2` (SYS_PIPE2 = 293)
/// Create an anonymous pipe with flags.
pub fn sys_pipe2(frame: &mut SyscallFrame) -> SyscallResult {
    sys_pipe(frame)
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

    let dentry = crate::fs::resolve_path(&path)?;
    if dentry.inode.inode_type != crate::fs::vfs::types::InodeType::Directory {
        return Err(SyscallError::ENOTDIR);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let mut proc = proc_arc.lock();
    proc.cwd = path;

    Ok(0)
}

/// `sys_fcntl` (SYS_FCNTL = 72)
/// Manipulate file descriptor properties.
pub fn sys_fcntl(frame: &mut SyscallFrame) -> SyscallResult {
    let fd = frame.arg1() as i32;
    let cmd = frame.arg2() as i32;

    if fd < 0 {
        return Err(SyscallError::EBADF);
    }

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(fd)?;

    match cmd {
        0 => Ok(proc.fd_table.alloc(file) as usize), // F_DUPFD
        _ => Ok(0),
    }
}

/// `sys_openat` (SYS_OPENAT = 257)
/// Open a file relative to directory descriptor.
pub fn sys_openat(frame: &mut SyscallFrame) -> SyscallResult {
    let _dfd = frame.arg1() as i32;
    let path_ptr = frame.arg2() as *const u8;
    let flags = frame.arg3() as u32;

    let path = unsafe { read_user_string(path_ptr, 256)? };

    let dentry = match crate::fs::resolve_path(&path) {
        Ok(d) => d,
        Err(crate::fs::vfs::types::VfsError::NotFound) if (flags & O_CREAT) != 0 => {
            crate::fs::create_file(&path)?
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

/// `sys_newfstatat` (SYS_NEWFSTATAT = 262)
/// Get file status relative to directory descriptor.
pub fn sys_newfstatat(frame: &mut SyscallFrame) -> SyscallResult {
    let _dfd = frame.arg1() as i32;
    let path_ptr = frame.arg2() as *const u8;
    let statbuf = frame.arg3() as *mut LinuxStat;

    if !is_user_ptr_valid(statbuf as u64, core::mem::size_of::<LinuxStat>()) {
        return Err(SyscallError::EFAULT);
    }

    let path = unsafe { read_user_string(path_ptr, 256)? };
    let vfs_stat = crate::fs::stat(&path)?;

    let linux_stat = copy_to_linux_stat(&vfs_stat);
    // SAFETY: Writing stat struct to user statbuf after validation.
    unsafe {
        core::ptr::write_volatile(statbuf, linux_stat);
    }

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
