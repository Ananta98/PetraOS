// SPDX-License-Identifier: GPL-2.0

//! System calls for reading directory entries: `getdents`, `getdents64`.

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use ostd::arch::cpu::context::UserContext;

use crate::{
    api::errno::{Errno, Result},
    fs::vfs::{File, Mode},
    proc::thread::Thread,
    syscall::fs::utils::UserPtr,
};

pub const DT_UNKNOWN: u8 = 0;
pub const DT_FIFO: u8 = 1;
pub const DT_CHR: u8 = 2;
pub const DT_DIR: u8 = 4;
pub const DT_BLK: u8 = 6;
pub const DT_REG: u8 = 8;
pub const DT_LNK: u8 = 10;
pub const DT_SOCK: u8 = 12;

fn mode_to_dtype(mode: Mode) -> u8 {
    if mode.contains(Mode::DIR) {
        DT_DIR
    } else if mode.contains(Mode::FILE) {
        DT_REG
    } else if mode.contains(Mode::LINK) {
        DT_LNK
    } else if mode.contains(Mode::CHAR) {
        DT_CHR
    } else if mode.contains(Mode::BLOCK) {
        DT_BLK
    } else if mode.contains(Mode::FIFO) {
        DT_FIFO
    } else if mode.contains(Mode::SOCKET) {
        DT_SOCK
    } else {
        DT_UNKNOWN
    }
}

/// Directory listing shared by `getdents`/`getdents64`: validates the fd is a
/// directory, reads the file's entry list, synthesizes `.`/`..`, and resolves
/// each child's inode number and type.
fn collect_dir_entries(file: &File) -> Result<Vec<(String, u64, u8)>> {
    if !file.inode.is_dir() {
        return Err(Errno::ENOTDIR);
    }

    let entries_raw = file.readdir()?;
    let raw_str = core::str::from_utf8(&entries_raw).map_err(|_| Errno::EIO)?;
    let lines: Vec<&str> = raw_str.lines().filter(|s| !s.is_empty()).collect();

    let mut all_entries: Vec<(String, u64, u8)> = Vec::new();
    if !lines.contains(&".") {
        all_entries.push((".".into(), file.inode.id as u64, DT_DIR));
    }
    if !lines.contains(&"..") {
        all_entries.push(("..".into(), file.inode.id as u64, DT_DIR));
    }

    for name in &lines {
        if *name == "." || *name == ".." {
            continue;
        }
        let (child_ino, child_type) = match file.inode.node_ops.lookup(name) {
            Ok(child) => {
                let d_type = mode_to_dtype(child.attr.read().mode);
                (child.id as u64, d_type)
            }
            Err(_) => (1, DT_UNKNOWN),
        };
        all_entries.push(((*name).into(), child_ino, child_type));
    }
    Ok(all_entries)
}

/// Shared getdents preamble: fd validation, directory-entry collection, and
/// `Ok(None)` for the trivially empty call (`count == 0`).
#[allow(clippy::type_complexity)]
fn prepare(
    fd: i32,
    count: usize,
) -> Result<
    Option<(
        Arc<crate::proc::Proc>,
        Arc<File>,
        Vec<(String, u64, u8)>,
    )>,
> {
    if fd < 0 {
        return Err(Errno::EBADF);
    }
    if count == 0 {
        return Ok(None);
    }
    let proc = Thread::proc_or_esrch()?;
    let file = proc.file(fd)?;
    let entries = collect_dir_entries(&file)?;
    Ok(Some((proc, file, entries)))
}

/// System call `getdents64(unsigned int fd, struct linux_dirent64 *dirp, unsigned int count)` (Syscall #217).
pub fn sys_getdents64(
    fd: i32,
    dirp_ptr: usize,
    count: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let Some((proc, file, all_entries)) = prepare(fd, count)? else {
        return Ok(0);
    };

    let mut offset_guard = file.offset.lock();
    let mut pos = *offset_guard as usize;
    if pos >= all_entries.len() {
        return Ok(0);
    }

    let mut written = 0;
    let vmar = proc.vmspace();

    while pos < all_entries.len() {
        let (name, ino, d_type) = all_entries[pos].clone();
        let name_bytes = name.as_bytes();
        let unaligned_len = 19 + name_bytes.len() + 1;
        let reclen = (unaligned_len + 7) & !7;

        if written + reclen > count {
            if written == 0 {
                return Err(Errno::EINVAL);
            }
            break;
        }

        let next_off = (pos + 1) as i64;
        let mut record = alloc::vec![0u8; reclen];
        record[0..8].copy_from_slice(&ino.to_ne_bytes());
        record[8..16].copy_from_slice(&next_off.to_ne_bytes());
        record[16..18].copy_from_slice(&(reclen as u16).to_ne_bytes());
        record[18] = d_type;
        record[19..19 + name_bytes.len()].copy_from_slice(name_bytes);
        record[19 + name_bytes.len()] = 0;

        UserPtr::new(dirp_ptr + written).write_bytes(&vmar, &record)?;
        written += reclen;
        pos += 1;
    }

    *offset_guard = pos as u64;
    Ok(written)
}

/// System call `getdents(unsigned int fd, struct linux_dirent *dirp, unsigned int count)` (Syscall #78).
pub fn sys_getdents(
    fd: i32,
    dirp_ptr: usize,
    count: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let Some((proc, file, all_entries)) = prepare(fd, count)? else {
        return Ok(0);
    };

    let mut offset_guard = file.offset.lock();
    let mut pos = *offset_guard as usize;
    if pos >= all_entries.len() {
        return Ok(0);
    }

    let mut written = 0;
    let vmar = proc.vmspace();

    while pos < all_entries.len() {
        let (name, ino, d_type) = all_entries[pos].clone();
        let name_bytes = name.as_bytes();
        let unaligned_len = 18 + name_bytes.len() + 1 + 1;
        let reclen = (unaligned_len + 7) & !7;

        if written + reclen > count {
            if written == 0 {
                return Err(Errno::EINVAL);
            }
            break;
        }

        let next_off = pos + 1;
        let mut record = alloc::vec![0u8; reclen];
        record[0..8].copy_from_slice(&(ino as usize).to_ne_bytes());
        record[8..16].copy_from_slice(&next_off.to_ne_bytes());
        record[16..18].copy_from_slice(&(reclen as u16).to_ne_bytes());
        record[18..18 + name_bytes.len()].copy_from_slice(name_bytes);
        record[18 + name_bytes.len()] = 0;
        record[reclen - 1] = d_type;

        UserPtr::new(dirp_ptr + written).write_bytes(&vmar, &record)?;
        written += reclen;
        pos += 1;
    }

    *offset_guard = pos as u64;
    Ok(written)
}
