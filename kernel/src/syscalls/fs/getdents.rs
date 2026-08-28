//! System calls for reading directory entries (`getdents64`).

use super::*;
use crate::arch::syscall::syscall::SyscallFrame;
use crate::fs::vfs::types::InodeType;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr};
use alloc::string::String;
use alloc::vec::Vec;

pub const DT_UNKNOWN: u8 = 0;
pub const DT_FIFO: u8 = 1;
pub const DT_CHR: u8 = 2;
pub const DT_DIR: u8 = 4;
pub const DT_BLK: u8 = 6;
pub const DT_REG: u8 = 8;
pub const DT_LNK: u8 = 10;
pub const DT_SOCK: u8 = 12;

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
