// SPDX-License-Identifier: GPL-2.0

//! The `stat`, `fstat`, `lstat`, and `newfstatat` system calls
//! (Syscalls #4, #5, #6, and #262 on x86_64).

use alloc::sync::Arc;
use ostd::arch::cpu::context::UserContext;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::{
    uapi::errno::{Errno, Result},
    fs::vfs::{INode, LookupFlags},
    proc::thread::Thread,
    syscall::fs::utils::{
        AT_EMPTY_PATH, AT_FDCWD, AT_SYMLINK_NOFOLLOW, UserCStr, UserPtr, resolve_at_start,
    },
    vm::vmar::Vmar,
};

/// `STATX_*` mask bits.
pub const STATX_TYPE: u32 = 0x0000_0001;
pub const STATX_MODE: u32 = 0x0000_0002;
pub const STATX_NLINK: u32 = 0x0000_0004;
pub const STATX_UID: u32 = 0x0000_0008;
pub const STATX_GID: u32 = 0x0000_0010;
pub const STATX_ATIME: u32 = 0x0000_0020;
pub const STATX_MTIME: u32 = 0x0000_0040;
pub const STATX_CTIME: u32 = 0x0000_0080;
pub const STATX_INO: u32 = 0x0000_0100;
pub const STATX_SIZE: u32 = 0x0000_0200;
pub const STATX_BLOCKS: u32 = 0x0000_0400;
pub const STATX_BASIC_STATS: u32 = 0x0000_07ff;
pub const STATX_BTIME: u32 = 0x0000_0800;
pub const STATX_MNT_ID: u32 = 0x0000_1000;
pub const STATX_DIOALIGN: u32 = 0x0000_2000;
pub const STATX_ALL: u32 = 0x0000_0fff;

/// Linux x86_64 ABI compatible `struct stat` (144 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, FromBytes, IntoBytes, KnownLayout, Immutable)]
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
    /// Serializes the stat structure into raw byte representation safely using zerocopy.
    pub fn to_bytes(&self) -> [u8; 144] {
        zerocopy::transmute!(*self)
    }
}

/// Linux x86_64 ABI compatible `struct statx_timestamp`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct StatxTimestamp {
    pub tv_sec: i64,
    pub tv_nsec: u32,
    pub __reserved: i32,
}

impl StatxTimestamp {
    pub fn to_bytes(&self) -> [u8; 16] {
        zerocopy::transmute!(*self)
    }
}

/// Linux x86_64 ABI compatible `struct statx` (256 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct Statx {
    pub stx_mask: u32,
    pub stx_blksize: u32,
    pub stx_attributes: u64,
    pub stx_nlink: u32,
    pub stx_uid: u32,
    pub stx_gid: u32,
    pub stx_mode: u16,
    pub __spare0: [u16; 1],
    pub stx_ino: u64,
    pub stx_size: u64,
    pub stx_blocks: u64,
    pub stx_attributes_mask: u64,
    pub stx_atime: StatxTimestamp,
    pub stx_btime: StatxTimestamp,
    pub stx_ctime: StatxTimestamp,
    pub stx_mtime: StatxTimestamp,
    pub stx_rdev_major: u32,
    pub stx_rdev_minor: u32,
    pub stx_dev_major: u32,
    pub stx_dev_minor: u32,
    pub stx_mnt_id: u64,
    pub stx_dio_mem_align: u32,
    pub stx_dio_offset_align: u32,
    pub __spare3: [u64; 12],
}

impl Statx {
    pub fn to_bytes(&self) -> [u8; 256] {
        zerocopy::transmute!(*self)
    }
}

fn dev_major(dev: u64) -> u32 {
    (((dev >> 8) & 0xfff) | ((dev >> 32) & !0xfff)) as u32
}

fn dev_minor(dev: u64) -> u32 {
    ((dev & 0xff) | ((dev >> 12) & !0xff)) as u32
}

pub fn statx_inode_to_user(
    inode: &Arc<INode>,
    statbuf_ptr: usize,
    vmar: &Vmar,
    _mask: u32,
) -> Result<()> {
    let attr = inode
        .node_ops
        .getattr()
        .unwrap_or_else(|_| *inode.attr.read());
    let times = inode.times();

    let statx = Statx {
        stx_mask: STATX_BASIC_STATS,
        stx_blksize: 4096,
        stx_attributes: 0,
        stx_nlink: attr.nlink as u32,
        stx_uid: attr.uid,
        stx_gid: attr.gid,
        stx_mode: (attr.mode.bits() & 0xffff) as u16,
        __spare0: [0],
        stx_ino: inode.id as u64,
        stx_size: attr.size as u64,
        stx_blocks: attr.size.div_ceil(512) as u64,
        stx_attributes_mask: 0,
        stx_atime: StatxTimestamp {
            tv_sec: times.atime as i64,
            tv_nsec: 0,
            __reserved: 0,
        },
        stx_btime: StatxTimestamp::default(),
        stx_ctime: StatxTimestamp {
            tv_sec: times.ctime as i64,
            tv_nsec: 0,
            __reserved: 0,
        },
        stx_mtime: StatxTimestamp {
            tv_sec: times.mtime as i64,
            tv_nsec: 0,
            __reserved: 0,
        },
        stx_rdev_major: dev_major(attr.rdev),
        stx_rdev_minor: dev_minor(attr.rdev),
        stx_dev_major: dev_major(1),
        stx_dev_minor: dev_minor(1),
        stx_mnt_id: 0,
        stx_dio_mem_align: 0,
        stx_dio_offset_align: 0,
        __spare3: [0; 12],
    };

    UserPtr::new(statbuf_ptr).write_val(vmar, statx)
}
pub fn stat_inode_to_user(inode: &Arc<INode>, statbuf_ptr: usize, vmar: &Vmar) -> Result<()> {
    let attr = inode
        .node_ops
        .getattr()
        .unwrap_or_else(|_| *inode.attr.read());
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

    UserPtr::new(statbuf_ptr).write_val(vmar, stat)
}

/// System call `newfstatat(int dirfd, const char *pathname, struct stat *statbuf, int flags)` (Syscall #262).
pub fn sys_newfstatat(
    dirfd: i32,
    path_ptr: usize,
    statbuf_ptr: usize,
    flags: i32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();

    let inode = if path_ptr == 0 {
        if (flags & AT_EMPTY_PATH) == 0 {
            return Err(Errno::EINVAL);
        }
        if dirfd == AT_FDCWD {
            proc.cwd()?.dentry.get_inode().ok_or(Errno::ENOENT)?
        } else {
            proc.file(dirfd)?.inode.clone()
        }
    } else {
        let path = UserCStr::new(path_ptr).read_string(&vmar, 4096)?;
        if path.is_empty() {
            if (flags & AT_EMPTY_PATH) != 0 {
                if dirfd == AT_FDCWD {
                    proc.cwd()?.dentry.get_inode().ok_or(Errno::ENOENT)?
                } else {
                    proc.file(dirfd)?.inode.clone()
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
            let root_node = proc.root()?;
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
    let proc = Thread::proc_or_esrch()?;
    let file = proc.file(fd)?;
    stat_inode_to_user(&file.inode, statbuf_ptr, &proc.vmspace())?;
    Ok(0)
}



/// System call `statx(int dirfd, const char *pathname, int flags, unsigned int mask, struct statx *statbuf)` (Syscall #332).
pub fn sys_statx(
    dirfd: i32,
    path_ptr: usize,
    flags: i32,
    mask: u32,
    statbuf_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();

    let inode = if path_ptr == 0 {
        if (flags & AT_EMPTY_PATH) == 0 {
            return Err(Errno::EINVAL);
        }
        if dirfd == AT_FDCWD {
            proc.cwd()?.dentry.get_inode().ok_or(Errno::ENOENT)?
        } else {
            proc.file(dirfd)?.inode.clone()
        }
    } else {
        let path = UserCStr::new(path_ptr).read_string(&vmar, 4096)?;
        if path.is_empty() {
            if (flags & AT_EMPTY_PATH) != 0 {
                if dirfd == AT_FDCWD {
                    proc.cwd()?.dentry.get_inode().ok_or(Errno::ENOENT)?
                } else {
                    proc.file(dirfd)?.inode.clone()
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
            let root_node = proc.root()?;
            let path_node = crate::fs::lookup(root_node, start, &path, lookup_flags)?;
            path_node.dentry.get_inode().ok_or(Errno::ENOENT)?
        }
    };

    statx_inode_to_user(&inode, statbuf_ptr, &vmar, mask)?;
    Ok(0)
}
