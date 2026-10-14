// SPDX-License-Identifier: GPL-2.0

//! The `statfs` and `fstatfs` system calls (Syscalls #137 and #138 on x86_64).

use ostd::arch::cpu::context::UserContext;

use crate::{
    api::errno::{Errno, Result},
    fs::vfs::StatFs,
    proc::thread::Thread,
    syscall::fs::utils::{read_user_string, write_user_bytes},
};

/// Linux x86_64 ABI compatible `struct statfs` (120 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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
        let mut buf = [0u8; 120];
        buf[0..8].copy_from_slice(&self.f_type.to_ne_bytes());
        buf[8..16].copy_from_slice(&self.f_bsize.to_ne_bytes());
        buf[16..24].copy_from_slice(&self.f_blocks.to_ne_bytes());
        buf[24..32].copy_from_slice(&self.f_bfree.to_ne_bytes());
        buf[32..40].copy_from_slice(&self.f_bavail.to_ne_bytes());
        buf[40..48].copy_from_slice(&self.f_files.to_ne_bytes());
        buf[48..56].copy_from_slice(&self.f_ffree.to_ne_bytes());
        buf[56..60].copy_from_slice(&self.f_fsid[0].to_ne_bytes());
        buf[60..64].copy_from_slice(&self.f_fsid[1].to_ne_bytes());
        buf[64..72].copy_from_slice(&self.f_namelen.to_ne_bytes());
        buf[72..80].copy_from_slice(&self.f_frsize.to_ne_bytes());
        buf[80..88].copy_from_slice(&self.f_flags.to_ne_bytes());
        buf[88..96].copy_from_slice(&self.f_spare[0].to_ne_bytes());
        buf[96..104].copy_from_slice(&self.f_spare[1].to_ne_bytes());
        buf[104..112].copy_from_slice(&self.f_spare[2].to_ne_bytes());
        buf[112..120].copy_from_slice(&self.f_spare[3].to_ne_bytes());
        buf
    }
}

impl From<StatFs> for LinuxStatFs {
    fn from(s: StatFs) -> Self {
        Self {
            f_type: 0x01021994, // TMPFS_MAGIC
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
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();
    let path = read_user_string(&vmar, path_ptr, 4096)?;

    let statfs = crate::fs::vfs::statfs(&path)?;
    let linux_statfs = LinuxStatFs::from(statfs);
    write_user_bytes(&vmar, buf_ptr, &linux_statfs.to_bytes())?;

    Ok(0)
}

/// System call `fstatfs(int fd, struct statfs *buf)` (Syscall #138).
pub fn sys_fstatfs(fd: i32, buf_ptr: usize, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::current_proc().ok_or(Errno::ESRCH)?;
    let vmar = proc.vmspace();
    let file = proc.fd_table.lock().get(fd)?;

    let statfs = if let Some(path_node) = &file.path_node {
        path_node.mount.fs.statfs()?
    } else {
        StatFs {
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
    write_user_bytes(&vmar, buf_ptr, &linux_statfs.to_bytes())?;

    Ok(0)
}
