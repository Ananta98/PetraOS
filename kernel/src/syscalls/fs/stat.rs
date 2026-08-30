//! System calls for querying file and filesystem status (`stat`, `fstat`, `lstat`, `newfstatat`).

use super::*;
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::vfs::types::{LinuxStat, Stat};
use crate::syscalls::{SyscallError, SyscallResult, UserCStr, UserPtr};

pub(crate) fn copy_to_linux_stat(stat: &Stat) -> LinuxStat {
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
    let mut vfs_stat = vfs_stat;
    if vfs_stat.ino == 0 {
        vfs_stat.ino = file.dentry.inode.ino;
    }
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
