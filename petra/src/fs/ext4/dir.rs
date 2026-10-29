// SPDX-License-Identifier: GPL-2.0

//! Ext4 directory entry parsing and mutation over raw directory data.
//!
//! Directory data is a sequence of [`Dirent`]s in variable-length chapters
//! (`rec_len` chains, 4-byte aligned, never crossing a block boundary).
//! Deleted entries are marked `inode == 0` and their slack merged into the
//! previous entry. Directory blocks with `metadata_csum` terminate in a
//! fake 0xDE-type checksum dirent which this module skips transparently.

use ::alloc::string::String;
use ::alloc::vec::Vec;

use crate::uapi::errno::{Errno, Result};

/// Dirent file-type byte values.
pub mod ftype {
    pub const UNKNOWN: u8 = 0;
    pub const REGULAR: u8 = 1;
    pub const DIR: u8 = 2;
    pub const CHR: u8 = 3;
    pub const BLK: u8 = 4;
    pub const FIFO: u8 = 5;
    pub const SOCK: u8 = 6;
    pub const SYMLINK: u8 = 7;
}

/// Dirent type byte for the `metadata_csum` tail (fake entry).
pub const DIRTAIL_TYPE: u8 = 0xDE;

/// A parsed directory entry positioned inside the raw data.
#[derive(Clone, Debug)]
pub struct Dirent {
    pub ino: u32,
    pub name: String,
    pub rec_len: u16,
    pub file_type: u8,
    /// Byte offset of the entry in the directory file.
    pub offset: usize,
    /// Offset of the previous entry (usize::MAX for the first one).
    pub prev_offset: Option<usize>,
}

/// Aligns `n` up to a 4-byte dirent boundary.
fn align4(n: usize) -> usize {
    (n + 3) & !3
}

/// Parses the whole directory blob into entries, stopping at the sum-check
/// tail (0xDE-type fake dirent) or at garbage.
///
/// `block_size` bounds parsing so entries never appear to cross blocks.
pub fn parse_all(dir_data: &[u8], block_size: usize) -> Vec<Dirent> {
    let mut out = Vec::new();
    let mut off = 0usize;
    let mut prev: Option<usize> = None;

    while off + 8 <= dir_data.len() {
        // A checksum tail dirent: inode 0, file_type 0xDE.
        if dir_data[off + 7] == DIRTAIL_TYPE
            && dir_data[off] == 0
            && dir_data[off + 1] == 0
            && dir_data[off + 2] == 0
            && dir_data[off + 3] == 0
        {
            break;
        }
        let ino = u32::from_le_bytes([
            dir_data[off],
            dir_data[off + 1],
            dir_data[off + 2],
            dir_data[off + 3],
        ]);
        let rec_len = u16::from_le_bytes([dir_data[off + 4], dir_data[off + 5]]) as usize;
        let name_len = dir_data[off + 6] as usize;
        let file_type = dir_data[off + 7];

        if rec_len < 8 {
            break;
        }
        // Do not let an entry appear to cross a block boundary.
        let block_end = ((off / block_size) + 1) * block_size;
        if off + rec_len > block_end {
            break;
        }

        let name_bytes: Vec<u8> = dir_data[off + 8..(off + 8 + name_len).min(off + rec_len)]
            .iter()
            .copied()
            .collect();
        let name = String::from_utf8_lossy(&name_bytes).into_owned();

        out.push(Dirent {
            ino,
            name,
            rec_len: rec_len as u16,
            file_type,
            offset: off,
            prev_offset: prev,
        });
        prev = Some(off);
        off += rec_len;
    }
    out
}

/// The byte offset of the scan-sum tail dirent for a full block, as `None`
/// when there is none (legacy directory layout).
pub fn tail_offset(dir_data: &[u8], offset: usize) -> Option<usize> {
    if offset + 8 <= dir_data.len()
        && dir_data[offset + 7] == DIRTAIL_TYPE
        && dir_data[offset..offset + 4] == [0, 0, 0, 0]
    {
        Some(offset)
    } else {
        None
    }
}

/// Serializes one dirent at `offset` in `dir_data`.
pub fn write_dirent(
    dir_data: &mut [u8],
    offset: usize,
    ino: u32,
    file_type: u8,
    name: &str,
    rec_len: u16,
) {
    dir_data[offset..offset + 4].copy_from_slice(&ino.to_le_bytes());
    dir_data[offset + 4..offset + 6].copy_from_slice(&rec_len.to_le_bytes());
    dir_data[offset + 6] = name.len() as u8;
    dir_data[offset + 7] = file_type;
    dir_data[offset + 8..offset + 8 + name.len()].copy_from_slice(name.as_bytes());
}

/// Searches for an entry by name, returning its inode and type.
pub fn find(entries: &[Dirent], name: &str) -> Option<(u32, u8)> {
    entries
        .iter()
        .filter(|e| e.ino != 0)
        .find(|e| e.name == name)
        .map(|e| (e.ino, e.file_type))
}

/// Counts subdirectory entries for the `links_count` bookkeeping.
pub fn count_subdirs(entries: &[Dirent]) -> u32 {
    entries
        .iter()
        .filter(|e| e.ino != 0 && e.file_type == ftype::DIR)
        .count() as u32
}

/// List of name/ino pairs suitable for readdir().
pub fn listing(entries: &[Dirent]) -> Vec<(String, u32, u8)> {
    entries
        .iter()
        .filter(|e| e.ino != 0)
        .map(|e| (e.name.clone(), e.ino, e.file_type))
        .collect()
}

/// Deletes an entry by merging its rec_len into the previous one (or
/// marking it as a free slot when it is first in the block).
pub fn delete_entry(dir_data: &mut [u8], entries: &[Dirent], target: &str) -> Result<(u32, u8)> {
    let entry = entries
        .iter()
        .find(|e| e.ino != 0 && e.name == target)
        .cloned()
        .ok_or(Errno::ENOENT)?;

    // Never remove "." or ".." through this path.
    if target == "." || target == ".." {
        return Err(Errno::EINVAL);
    }

    write_dirent(dir_data, entry.offset, 0, 0, "", entry.rec_len);
    if let Some(prev_off) = entry.prev_offset {
        if prev_off + 8 <= dir_data.len() {
            let old_rec = u16::from_le_bytes([dir_data[prev_off + 4], dir_data[prev_off + 5]]);
            // Only merge when the previous slot is a live entry; otherwise the
            // current free slot remains available (dir appended by caller).
            let merged = old_rec.saturating_add(entry.rec_len);
            dir_data[prev_off + 4..prev_off + 6].copy_from_slice(&merged.to_le_bytes());
        }
    }
    Ok((entry.ino, entry.file_type))
}

/// Marks the "." reference of `dir_data` to `parent`.
pub fn set_dotdot(dir_data: &mut [u8], entries: &[Dirent], parent: u32) -> Result<()> {
    let e = entries
        .iter()
        .find(|e| e.ino != 0 && e.name == "..")
        .cloned()
        .ok_or(Errno::EIO)?;
    dir_data[e.offset..e.offset + 4].copy_from_slice(&parent.to_le_bytes());
    Ok(())
}

/// Inserts a new entry, re-using a free slot or expanding the final slack.
///
/// Returns `Err(ENOSPC)` when no space is available in the current blob (the
/// caller then grows the directory file by one block).
pub fn insert_entry(
    dir_data: &mut [u8],
    block_size: usize,
    name: &str,
    child_ino: u32,
    file_type: u8,
    dir_end: usize,
) -> Result<()> {
    if name.len() > 255 {
        return Err(Errno::ENAMETOOLONG);
    }
    let needed: usize = ((8 + name.len()) + 3) & !3;

    let mut off = 0usize;
    while off + 8 <= dir_data.len() {
        let block_end = ((off / block_size) + 1) * block_size;
        // Checksum tail terminating the block: stop before it.
        if tail_offset(dir_data, off).is_some() {
            break;
        }
        let ino = u32::from_le_bytes([
            dir_data[off],
            dir_data[off + 1],
            dir_data[off + 2],
            dir_data[off + 3],
        ]);
        let rec_len = u16::from_le_bytes([dir_data[off + 4], dir_data[off + 5]]) as usize;
        if rec_len == 0 {
            break;
        }

        // Free slot with enough room?
        if ino == 0 && rec_len >= needed + 8 {
            // Shrink this slot to `needed` and add a remainder marker. A
            // remainder of 8 bytes may turn into a tail; < 8 cannot exist,
            // so remaining space becomes a free marker at `off + needed`.
            write_dirent(dir_data, off, child_ino, file_type, name, needed as u16);
            let remaining = rec_len - needed;
            if remaining >= 8 && off + needed + remaining <= block_end {
                write_dirent(dir_data, off + needed, 0, 0, "", remaining as u16);
            } else if remaining > 0 {
                // Expand this entry to consume the rest of the slot.
                let grown = needed + remaining;
                dir_data[off + 4..off + 6].copy_from_slice(&(grown as u16).to_le_bytes());
            }
            return Ok(());
        }

        // Last live entry with enlargeable rec_len into block slack?
        let next_offset = off + rec_len;
        if next_offset >= dir_end || next_offset >= block_end {
            let gapped = block_end - off;
            if gapped >= needed + 8 && ino != 0 {
                // Shrink and append.
                dir_data[off + 4..off + 6].copy_from_slice(&(needed as u16).to_le_bytes());
                write_dirent(
                    dir_data,
                    off + needed,
                    child_ino,
                    file_type,
                    name,
                    (gapped - needed) as u16,
                );
                return Ok(());
            }
            // If the next scan lands past `dir_end`, no more fit.
            off += rec_len;
            if off >= dir_end || off >= block_end {
                break;
            }
            continue;
        }
        off += rec_len;
        // A directory of legacy layout may reach the end without slack.
        if off + 8 > dir_data.len() {
            break;
        }
    }
    Err(Errno::ENOSPC)
}

/// Updates the inode reference of an entry.
pub fn update_entry(
    dir_data: &mut [u8],
    entries: &[Dirent],
    target: &str,
    new_ino: u32,
) -> Result<()> {
    let entry = entries
        .iter()
        .find(|e| e.ino != 0 && e.name == target)
        .cloned()
        .ok_or(Errno::ENOENT)?;
    dir_data[entry.offset..entry.offset + 4].copy_from_slice(&new_ino.to_le_bytes());
    Ok(())
}

/// Validates directory content invariants loosely (useful on mount).
pub fn sanity(entries: &[Dirent]) -> Result<()> {
    for e in entries {
        if e.ino == 0 && e.file_type != 0 {
            return Err(Errno::EIO);
        }
    }
    Ok(())
}
