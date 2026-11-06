// SPDX-License-Identifier: GPL-2.0

//! The `statfs` and `fstatfs` system calls (Syscalls #137 and #138 on x86_64).

use ostd::arch::cpu::context::UserContext;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::{
    uapi::errno::Result,
    fs::vfs::StatFs,
    proc::thread::Thread,
    syscall::fs::utils::{UserCStr, UserPtr},
};

/// Linux x86_64 ABI compatible `struct statfs` (120 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct LinuxStatFs {
    pub f_type: i64,
    pub f_bsize: i64,
    pub f_blocks: u64,
    pub f_bfree: u64,
    pub f_bavail: u64,
    pub f_files: u64,
    pub f_ffree: u64,
    pub f_fsid: [i32; 2],
    pub f_namelen: i64,
    pub f_frsize: i64,
    pub f_flags: i64,
    pub f_spare: [i64; 4],
}

impl LinuxStatFs {
    pub fn to_bytes(&self) -> [u8; 120] {
        zerocopy::transmute!(*self)
    }
}

impl From<StatFs> for LinuxStatFs {
    fn from(s: StatFs) -> Self {
        Self {
            f_type: if s.f_type != 0 { s.f_type } else { 0x01021994 },
            f_bsize: s.bsize as i64,
            f_blocks: s.blocks,
            f_bfree: s.bfree,
            f_bavail: s.bavail,
            f_files: s.files,
            f_ffree: s.ffree,
            f_fsid: [0, 0],
            f_namelen: s.namelen as i64,
            f_frsize: s.bsize as i64,
            f_flags: 0,
            f_spare: [0; 4],
        }
    }
}

/// System call `statfs(const char *path, struct statfs *buf)` (Syscall #137).
pub fn sys_statfs(path_ptr: usize, buf_ptr: usize, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();
    let path = UserCStr::new(path_ptr).read_string(&vmar, 4096)?;

    let statfs = crate::fs::vfs::statfs(&path)?;
    let linux_statfs = LinuxStatFs::from(statfs);
    UserPtr::new(buf_ptr).write_val(&vmar, linux_statfs)?;

    Ok(0)
}

/// System call `fstatfs(int fd, struct statfs *buf)` (Syscall #138).
pub fn sys_fstatfs(fd: i32, buf_ptr: usize, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();
    let file = proc.file(fd)?;

    let statfs = if let Some(path_node) = &file.path_node {
        path_node.mount.fs.statfs()?
    } else {
        StatFs {
            f_type: 0x01021994,
            bsize: 4096,
            blocks: 1024 * 1024,
            bfree: 1024 * 1024,
            bavail: 1024 * 1024,
            files: 1024 * 1024,
            ffree: 1024 * 1024,
            namelen: 255,
        }
    };

    let linux_statfs = LinuxStatFs::from(statfs);
    UserPtr::new(buf_ptr).write_val(&vmar, linux_statfs)?;

    Ok(0)
}
