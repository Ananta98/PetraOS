// SPDX-License-Identifier: GPL-2.0

//! The `stat`, `fstat`, `lstat`, and `newfstatat` system calls
//! (Syscalls #4, #5, #6, and #262 on x86_64).

use alloc::sync::Arc;
use ostd::arch::cpu::context::UserContext;

use crate::{
    api::errno::{Errno, Result},
    fs::vfs::{INode, LookupFlags},
    proc::thread::Thread,
    syscall::fs::utils::{
        AT_EMPTY_PATH, AT_FDCWD, AT_SYMLINK_NOFOLLOW, read_user_string, resolve_at_start,
        write_user_bytes,
    },
    vm::vmar::Vmar,
};

/// Linux x86_64 ABI compatible `struct stat` (144 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stat {
    pub st_dev: u64,
    pub st_ino: u64,
    pub st_nlink: u64,
    pub st_mode: u32,
    pub st_uid: u32,
    pub st_gid: u32,
    pub __pad0: u32,
    pub st_rdev: u64,
    pub st_size: i64,
    pub st_blksize: i64,
    pub st_blocks: i64,
    pub st_atime: u64,
    pub st_atime_nsec: u64,
    pub st_mtime: u64,
    pub st_mtime_nsec: u64,
    pub st_ctime: u64,
    pub st_ctime_nsec: u64,
    pub __glibc_reserved: [i64; 3],
}

impl Stat {
    /// Serializes the stat structure into raw byte representation safely without unsafe.
    pub fn to_bytes(&self) -> [u8; 144] {
        let mut buf = [0u8; 144];
        buf[0..8].copy_from_slice(&self.st_dev.to_ne_bytes());
        buf[8..16].copy_from_slice(&self.st_ino.to_ne_bytes());
        buf[16..24].copy_from_slice(&self.st_nlink.to_ne_bytes());
        buf[24..28].copy_from_slice(&self.st_mode.to_ne_bytes());
        buf[28..32].copy_from_slice(&self.st_uid.to_ne_bytes());
        buf[32..36].copy_from_slice(&self.st_gid.to_ne_bytes());
        buf[36..40].copy_from_slice(&self.__pad0.to_ne_bytes());
        buf[40..48].copy_from_slice(&self.st_rdev.to_ne_bytes());
        buf[48..56].copy_from_slice(&self.st_size.to_ne_bytes());
        buf[56..64].copy_from_slice(&self.st_blksize.to_ne_bytes());
        buf[64..72].copy_from_slice(&self.st_blocks.to_ne_bytes());
        buf[72..80].copy_from_slice(&self.st_atime.to_ne_bytes());
        buf[80..88].copy_from_slice(&self.st_atime_nsec.to_ne_bytes());
        buf[88..96].copy_from_slice(&self.st_mtime.to_ne_bytes());
        buf[96..104].copy_from_slice(&self.st_mtime_nsec.to_ne_bytes());
        buf[104..112].copy_from_slice(&self.st_ctime.to_ne_bytes());
        buf[112..120].copy_from_slice(&self.st_ctime_nsec.to_ne_bytes());
        buf[120..128].copy_from_slice(&self.__glibc_reserved[0].to_ne_bytes());
        buf[128..136].copy_from_slice(&self.__glibc_reserved[1].to_ne_bytes());
        buf[136..144].copy_from_slice(&self.__glibc_reserved[2].to_ne_bytes());
        buf
    }
}

/// Helper to serialize inode attributes to userspace `struct stat`.
pub fn stat_inode_to_user(inode: &Arc<INode>, statbuf_ptr: usize, vmar: &Vmar) -> Result<()> {
    let attr = inode.node_ops.getattr().unwrap_or_else(|_| *inode.attr.read());
    let times = inode.times();

    let stat = Stat {
        st_dev: 1,
        st_ino: inode.id as u64,
        st_nlink: attr.nlink as u64,
        st_mode: attr.mode.bits(),
        st_uid: attr.uid,
        st_gid: attr.gid,
        __pad0: 0,
        st_rdev: attr.rdev,
        st_size: attr.size as i64,
        st_blksize: 4096,
        st_blocks: attr.size.div_ceil(512) as i64,
        st_atime: times.atime,
        st_atime_nsec: 0,
        st_mtime: times.mtime,
        st_mtime_nsec: 0,
        st_ctime: times.ctime,
        st_ctime_nsec: 0,
        __glibc_reserved: [0; 3],
    };

    write_user_bytes(vmar, statbuf_ptr, &stat.to_bytes())
}

/// System call `newfstatat(int dirfd, const char *pathname, struct stat *statbuf, int flags)` (Syscall #262).
pub fn sys_newfstatat(
    dirfd: i32,
    path_ptr: usize,
    statbuf_ptr: usize,
    flags: i32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();

    let inode = if path_ptr == 0 {
        if (flags & AT_EMPTY_PATH) == 0 {
            return Err(Errno::EINVAL);
        }
        if dirfd == AT_FDCWD {
            proc.cwd()?.dentry.get_inode().ok_or(Errno::ENOENT)?
        } else {
            proc.fd_table.lock().get(dirfd)?.inode.clone()
        }
    } else {
        let path = read_user_string(&vmar, path_ptr, 4096)?;
        if path.is_empty() {
            if (flags & AT_EMPTY_PATH) != 0 {
                if dirfd == AT_FDCWD {
                    proc.cwd()?.dentry.get_inode().ok_or(Errno::ENOENT)?
                } else {
                    proc.fd_table.lock().get(dirfd)?.inode.clone()
                }
            } else {
                return Err(Errno::ENOENT);
            }
        } else {
            let start = resolve_at_start(&proc, dirfd, &path)?;
            let lookup_flags = if (flags & AT_SYMLINK_NOFOLLOW) != 0 {
                LookupFlags::NO_FOLLOW
            } else {
                LookupFlags::empty()
            };
            let root_node = crate::fs::root()?;
            let path_node = crate::fs::lookup(root_node, start, &path, lookup_flags)?;
            path_node.dentry.get_inode().ok_or(Errno::ENOENT)?
        }
    };

    stat_inode_to_user(&inode, statbuf_ptr, &vmar)?;
    Ok(0)
}

/// System call `stat(const char *pathname, struct stat *statbuf)` (Syscall #4).
pub fn sys_stat(path_ptr: usize, statbuf_ptr: usize, ctx: &mut UserContext) -> Result<usize> {
    sys_newfstatat(AT_FDCWD, path_ptr, statbuf_ptr, 0, ctx)
}

/// System call `lstat(const char *pathname, struct stat *statbuf)` (Syscall #6).
pub fn sys_lstat(path_ptr: usize, statbuf_ptr: usize, ctx: &mut UserContext) -> Result<usize> {
    sys_newfstatat(AT_FDCWD, path_ptr, statbuf_ptr, AT_SYMLINK_NOFOLLOW, ctx)
}

/// System call `fstat(int fd, struct stat *statbuf)` (Syscall #5).
pub fn sys_fstat(fd: i32, statbuf_ptr: usize, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let file = proc.fd_table.lock().get(fd)?;
    stat_inode_to_user(&file.inode, statbuf_ptr, &proc.vmspace())?;
    Ok(0)
}
