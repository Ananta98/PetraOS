// SPDX-License-Identifier: GPL-2.0

//! System calls for reading directory entries: `getdents`, `getdents64`.

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use ostd::arch::cpu::context::UserContext;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::{
    uapi::errno::{Errno, Result},
    fs::vfs::{DEntry, File, Mode},
    proc::thread::Thread,
    syscall::fs::utils::UserPtr,
};

/// Fixed header for `struct linux_dirent64` (19 bytes).
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct Dirent64Header {
    pub d_ino: u64,
    pub d_off: i64,
    pub d_reclen: u16,
    pub d_type: u8,
}

/// Fixed header for `struct linux_dirent` (18 bytes).
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct DirentHeader {
    pub d_ino: usize,
    pub d_off: usize,
    pub d_reclen: u16,
}

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

fn resolve_child_entry(file: &File, name: &str) -> (u64, u8) {
    let cached = file
        .path_node
        .as_ref()
        .and_then(|pn| pn.dentry.lookup_child(name))
        .and_then(|d| d.get_inode());
    if let Some(child) = cached {
        return (child.id as u64, mode_to_dtype(child.attr.read().mode));
    }
    match file.inode.node_ops.lookup(name) {
        Ok(child) => {
            let ino = child.id as u64;
            let dtype = mode_to_dtype(child.attr.read().mode);
            if let Some(ref pn) = file.path_node {
                let dentry = Arc::new(DEntry::new(
                    name.into(),
                    Some(child),
                    Some(Arc::downgrade(&pn.dentry)),
                ));
                pn.dentry.add_child(dentry);
            }
            (ino, dtype)
        }
        Err(_) => (1, DT_UNKNOWN),
    }
}

/// Directory listing shared by `getdents`/`getdents64`: validates the fd is a
/// directory, reads the file's entry list, synthesizes `.`/`..`, and resolves
/// each child's inode number and type.
fn collect_dir_entries(file: &File) -> Result<Vec<(String, u64, u8)>> {
    if !file.inode.is_dir() {
        return Err(Errno::ENOTDIR);
    }

    if let Some(direct) = file.readdir_entries()? {
        let mut all_entries: Vec<(String, u64, u8)> = Vec::with_capacity(direct.len() + 2);
        all_entries.push((".".into(), file.inode.id as u64, DT_DIR));
        all_entries.push(("..".into(), file.inode.id as u64, DT_DIR));
        for (name, ino, dtype) in direct {
            if name == "." || name == ".." {
                continue;
            }
            all_entries.push((name, ino, dtype));
        }
        return Ok(all_entries);
    }

    let entries_raw = file.readdir()?;
    let raw_str = core::str::from_utf8(&entries_raw).map_err(|_| Errno::EIO)?;
    let lines: Vec<&str> = raw_str.lines().filter(|s| !s.is_empty()).collect();

    let mut all_entries: Vec<(String, u64, u8)> = Vec::with_capacity(lines.len() + 2);
    all_entries.push((".".into(), file.inode.id as u64, DT_DIR));
    all_entries.push(("..".into(), file.inode.id as u64, DT_DIR));

    for name in &lines {
        if *name == "." || *name == ".." {
            continue;
        }
        let (child_ino, child_type) = resolve_child_entry(file, name);
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
        Arc<Vec<(String, u64, u8)>>,
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
    if !file.inode.is_dir() {
        return Err(Errno::ENOTDIR);
    }
    let cached = file.dir_entries_cache.lock().clone();
    let entries = match cached {
        Some(list) => list,
        None => {
            let built = Arc::new(collect_dir_entries(&file)?);
            *file.dir_entries_cache.lock() = Some(Arc::clone(&built));
            built
        }
    };
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

    let mut kbuf = Vec::with_capacity(count.min(65536));
    let vmar = proc.vmspace();

    while pos < all_entries.len() {
        let (ref name, ino, d_type) = all_entries[pos];
        let name_bytes = name.as_bytes();
        let unaligned_len = 19 + name_bytes.len() + 1;
        let reclen = (unaligned_len + 7) & !7;

        if kbuf.len() + reclen > count {
            if kbuf.is_empty() {
                return Err(Errno::EINVAL);
            }
            break;
        }

        let next_off = (pos + 1) as i64;
        let base = kbuf.len();
        kbuf.resize(base + reclen, 0);
        let header = Dirent64Header {
            d_ino: ino,
            d_off: next_off,
            d_reclen: reclen as u16,
            d_type,
        };
        kbuf[base..base + 19].copy_from_slice(header.as_bytes());
        kbuf[base + 19..base + 19 + name_bytes.len()].copy_from_slice(name_bytes);

        pos += 1;
    }

    let written = kbuf.len();
    if written > 0 {
        UserPtr::new(dirp_ptr).write_bytes(&vmar, &kbuf)?;
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

    let mut kbuf = Vec::with_capacity(count.min(65536));
    let vmar = proc.vmspace();

    while pos < all_entries.len() {
        let (ref name, ino, d_type) = all_entries[pos];
        let name_bytes = name.as_bytes();
        let unaligned_len = 18 + name_bytes.len() + 1 + 1;
        let reclen = (unaligned_len + 7) & !7;

        if kbuf.len() + reclen > count {
            if kbuf.is_empty() {
                return Err(Errno::EINVAL);
            }
            break;
        }

        let next_off = pos + 1;
        let base = kbuf.len();
        kbuf.resize(base + reclen, 0);
        let header = DirentHeader {
            d_ino: ino as usize,
            d_off: next_off,
            d_reclen: reclen as u16,
        };
        kbuf[base..base + 18].copy_from_slice(header.as_bytes());
        kbuf[base + 18..base + 18 + name_bytes.len()].copy_from_slice(name_bytes);
        kbuf[base + reclen - 1] = d_type;

        pos += 1;
    }

    let written = kbuf.len();
    if written > 0 {
        UserPtr::new(dirp_ptr).write_bytes(&vmar, &kbuf)?;
    }
    *offset_guard = pos as u64;
    Ok(written)
}
