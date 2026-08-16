use super::{SyscallError, SyscallResult, is_user_ptr_valid, read_user_string};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::vfs::types::{File, InodeType, LinuxStat, O_CREAT, O_RDONLY, O_WRONLY, SeekWhence, Stat};
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
        core::ptr::write_unaligned(statbuf, linux_stat);
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
        core::ptr::write_unaligned(statbuf, linux_stat);
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
        core::ptr::write_unaligned(pipefd, r_fd);
        core::ptr::write_unaligned(pipefd.add(1), w_fd);
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
        core::ptr::write_unaligned(pipefd, r_fd);
        core::ptr::write_unaligned(pipefd.add(1), w_fd);
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
        core::ptr::write_unaligned(buf.add(cwd_bytes.len()), 0);
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

fn resolve_at_path(dfd: i32, path: &str) -> Result<alloc::string::String, SyscallError> {
    if path.starts_with('/') {
        Ok(crate::fs::normalize_path("/", path))
    } else if dfd == -100 || dfd as u32 == 0xffffff9c {
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

/// `sys_openat` (SYS_OPENAT = 257)
/// Open a file relative to directory descriptor.
pub fn sys_openat(frame: &mut SyscallFrame) -> SyscallResult {
    let dfd = frame.arg1() as i32;
    let path_ptr = frame.arg2() as *const u8;
    let flags = frame.arg3() as u32;

    let path = unsafe { read_user_string(path_ptr, 256)? };
    let full_path = resolve_at_path(dfd, &path)?;

    let dentry = match crate::fs::resolve_path(&full_path) {
        Ok(d) => d,
        Err(crate::fs::vfs::types::VfsError::NotFound) if (flags & O_CREAT) != 0 => {
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

/// `sys_access` (SYS_ACCESS = 21)
/// Check user's permissions for a file.
pub fn sys_access(frame: &mut SyscallFrame) -> SyscallResult {
    let path_ptr = frame.arg1() as *const u8;
    let mode = frame.arg2() as i32;

    if mode < 0 || mode > 7 {
        return Err(SyscallError::EINVAL);
    }

    let path = unsafe { read_user_string(path_ptr, 256)? };
    let _dentry = crate::fs::resolve_path(&path)?;

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
        core::ptr::write_unaligned(statbuf, linux_stat);
    }

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
    let mut all_entries: alloc::vec::Vec<(alloc::string::String, u64, u8)> = alloc::vec::Vec::new();

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
            core::ptr::write_unaligned(dest as *mut u64, ino);
            // Write d_off (i64 at offset 8)
            core::ptr::write_unaligned(dest.add(8) as *mut i64, off);
            // Write d_reclen (u16 at offset 16)
            core::ptr::write_unaligned(dest.add(16) as *mut u16, reclen as u16);
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
