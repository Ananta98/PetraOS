//! Initramfs unpacker.
//!
//! The bootloader loads the initramfs archive (configured via `OSDK.toml`'s
//! `initramfs` option) into memory and exposes it through
//! [`ostd::boot::boot_info`].  This module parses the `newc` (SVR4) cpio
//! format and populates the root filesystem with the archived files so that
//! user-space programs (e.g. `/bin/bash`) are present before PID 1 is spawned.

use crate::fs::vfs::{Dentry, InodeOps, Result};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use ostd::Error;

/// Magic of a `newc` cpio entry header.
const CPIO_NEWC_MAGIC: &[u8; 6] = b"070701";
/// Header size of a `newc` cpio entry (13 8-digit hexadecimal fields).
const CPIO_HEADER_SIZE: usize = 110;
/// Name of the trailing cpio entry.
const CPIO_TRAILER: &str = "TRAILER!!!";

/// File-type mask inside a cpio mode word.
const S_IFMT: u32 = 0o170000;
const S_IFREG: u32 = 0o100000;
const S_IFDIR: u32 = 0o040000;
const S_IFLNK: u32 = 0o120000;

/// Unpack the bootloader-provided initramfs archive into the root filesystem.
///
/// Returns `Ok(())` (without doing anything) when no initramfs was provided
/// by the bootloader.
pub fn unpack_initramfs(root: &Arc<Dentry>) -> Result<()> {
    let Some(archive) = ostd::boot::boot_info().initramfs else {
        ostd::early_println!("[initramfs] WARNING: No initramfs archive provided by bootloader!");
        return Ok(());
    };

    ostd::early_println!("[initramfs] Unpacking {} bytes...", archive.len());

    let mut offset = 0usize;
    while offset + CPIO_HEADER_SIZE <= archive.len() {
        let header = &archive[offset..offset + CPIO_HEADER_SIZE];
        if &header[0..6] != CPIO_NEWC_MAGIC {
            return Err(Error::InvalidArgs);
        }

        let hex_field = |start: usize, end: usize| -> Result<usize> {
            let field =
                core::str::from_utf8(&header[start..end]).map_err(|_| Error::InvalidArgs)?;
            usize::from_str_radix(field, 16).map_err(|_| Error::InvalidArgs)
        };

        let mode = u32::try_from(hex_field(14, 22)?).map_err(|_| Error::InvalidArgs)?;
        let file_size = hex_field(54, 62)?;
        let name_size = hex_field(94, 102)?;

        let mut pos = offset + CPIO_HEADER_SIZE;
        let name_end = pos.checked_add(name_size).ok_or(Error::InvalidArgs)?;
        if name_end > archive.len() {
            return Err(Error::InvalidArgs);
        }
        let raw_name =
            String::from_utf8(archive[pos..name_end].to_vec()).map_err(|_| Error::InvalidArgs)?;
        pos = align4(name_end);

        // The cpio `newc` format includes a NUL terminator in the name
        // field (counted in `namesize`).  Strip it so that filesystem
        // entry names do not contain embedded NUL bytes — otherwise
        // later path resolution will fail to match them.
        let name = raw_name.trim_end_matches('\0');

        if name == CPIO_TRAILER {
            break;
        }

        let data_end = pos.checked_add(file_size).ok_or(Error::InvalidArgs)?;
        if data_end > archive.len() {
            return Err(Error::InvalidArgs);
        }
        let data = &archive[pos..data_end];
        offset = align4(data_end);

        unpack_entry(root, name, mode, data)?;
    }

    Ok(())
}

/// Create a single cpio entry (regular file, directory, or symlink) inside the
/// root filesystem, creating any missing parent directories along the way.
fn unpack_entry(root: &Arc<Dentry>, path: &str, mode: u32, data: &[u8]) -> Result<()> {
    let trimmed = path.trim_start_matches('/');
    if trimmed.is_empty() {
        return Ok(());
    }

    let components: Vec<&str> = trimmed.split('/').filter(|c| !c.is_empty()).collect();
    let (parent_components, leaf) = components.split_at(components.len() - 1);
    let leaf = leaf[0];

    let mut current: Arc<dyn InodeOps> = root.inode.clone();
    for component in parent_components {
        let child = match current.lookup(component) {
            Ok(child) => child,
            Err(_) => current.mkdir(component, 0o755)?,
        };
        current = child;
    }

    match mode & S_IFMT {
        S_IFREG => {
            let inode = current.create(leaf, mode & 0o7777)?;
            if !data.is_empty() {
                let mut file = inode.open(0)?;
                let mut offset = 0usize;
                file.write(data, &mut offset)?;
            }
        }
        S_IFDIR => {
            if current.lookup(leaf).is_err() {
                let _ = current.mkdir(leaf, mode & 0o7777)?;
            }
        }
        S_IFLNK => {
            if current.lookup(leaf).is_err() {
                let target = core::str::from_utf8(data).map_err(|_| Error::InvalidArgs)?;
                let _ = current.symlink(leaf, target)?;
            }
        }
        _ => {
            log::warn!("[initramfs] skipping unsupported entry '{}'", path);
        }
    }

    Ok(())
}

/// Round `value` up to the next 4-byte boundary.
fn align4(value: usize) -> usize {
    (value + 3) & !3
}
