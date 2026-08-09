use super::inode::{Ext2Inode, Ext2Volume};
use crate::fs::vfs::types::VfsError;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

/// Iterate over the directory entries of a directory inode and return their names.
pub fn ext2_readdir(volume: &Arc<Ext2Volume>, inode: &Ext2Inode) -> Result<Vec<String>, VfsError> {
    let mut entries = Vec::new();
    let mut offset = 0;
    let size = inode.size as usize;
    let mut block_buf = alloc::vec![0u8; size];
    volume.read_inode_data(inode, 0, &mut block_buf)?;

    while offset < size {
        if offset + 8 > size {
            break;
        }
        let ino = u32::from_le_bytes([
            block_buf[offset],
            block_buf[offset + 1],
            block_buf[offset + 2],
            block_buf[offset + 3],
        ]);
        let rec_len = u16::from_le_bytes([block_buf[offset + 4], block_buf[offset + 5]]) as usize;
        let name_len = block_buf[offset + 6] as usize;

        if rec_len == 0 {
            break;
        }

        if ino != 0 && offset + 8 + name_len <= size {
            let name_bytes = &block_buf[offset + 8..offset + 8 + name_len];
            let name = String::from_utf8_lossy(name_bytes).into_owned();
            entries.push(name);
        }

        offset += rec_len;
    }

    Ok(entries)
}

/// Look up a specific entry name within the directory and return its inode index.
pub fn ext2_lookup(
    volume: &Arc<Ext2Volume>,
    inode: &Ext2Inode,
    name: &str,
) -> Result<u32, VfsError> {
    let mut offset = 0;
    let size = inode.size as usize;
    let mut block_buf = alloc::vec![0u8; size];
    volume.read_inode_data(inode, 0, &mut block_buf)?;

    while offset < size {
        if offset + 8 > size {
            break;
        }
        let ino = u32::from_le_bytes([
            block_buf[offset],
            block_buf[offset + 1],
            block_buf[offset + 2],
            block_buf[offset + 3],
        ]);
        let rec_len = u16::from_le_bytes([block_buf[offset + 4], block_buf[offset + 5]]) as usize;
        let name_len = block_buf[offset + 6] as usize;

        if rec_len == 0 {
            break;
        }

        if ino != 0 && offset + 8 + name_len <= size {
            let name_bytes = &block_buf[offset + 8..offset + 8 + name_len];
            if name_bytes == name.as_bytes() {
                return Ok(ino);
            }
        }

        offset += rec_len;
    }

    Err(VfsError::NotFound)
}

/// Helper function to align record sizes to 4-byte boundaries.
fn align4(n: usize) -> usize {
    (n + 3) & !3
}

/// Check if a directory inode is empty (contains no entries other than "." and "..").
pub fn ext2_is_dir_empty(volume: &Arc<Ext2Volume>, inode: &Ext2Inode) -> Result<bool, VfsError> {
    let entries = ext2_readdir(volume, inode)?;
    for entry in entries {
        if entry != "." && entry != ".." {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Add a new directory entry into a directory inode.
pub fn ext2_add_entry(
    volume: &Arc<Ext2Volume>,
    dir_inode: &mut Ext2Inode,
    dir_ino: u32,
    name: &str,
    child_ino: u32,
    file_type: u8,
) -> Result<(), VfsError> {
    let name_bytes = name.as_bytes();
    let needed_len = align4(8 + name_bytes.len());

    let mut size = dir_inode.size as usize;
    if size == 0 {
        size = volume.sb.block_size as usize;
        dir_inode.size = size as u32;
    }

    let mut dir_buf = alloc::vec![0u8; size];
    volume.read_inode_data(dir_inode, 0, &mut dir_buf)?;

    let mut offset = 0;
    let mut inserted = false;

    while offset < size {
        if offset + 8 > size {
            break;
        }

        let ino = u32::from_le_bytes([
            dir_buf[offset],
            dir_buf[offset + 1],
            dir_buf[offset + 2],
            dir_buf[offset + 3],
        ]);
        let rec_len = u16::from_le_bytes([dir_buf[offset + 4], dir_buf[offset + 5]]) as usize;
        let name_len = dir_buf[offset + 6] as usize;

        if rec_len == 0 {
            let block_rem = volume.sb.block_size as usize - (offset % volume.sb.block_size as usize);
            let entry_rec_len = block_rem as u16;
            dir_buf[offset..offset + 4].copy_from_slice(&child_ino.to_le_bytes());
            dir_buf[offset + 4..offset + 6].copy_from_slice(&entry_rec_len.to_le_bytes());
            dir_buf[offset + 6] = name_bytes.len() as u8;
            dir_buf[offset + 7] = file_type;
            dir_buf[offset + 8..offset + 8 + name_bytes.len()].copy_from_slice(name_bytes);

            inserted = true;
            break;
        }

        let actual_len = if ino == 0 { 0 } else { align4(8 + name_len) };
        let space_available = rec_len - actual_len;

        if space_available >= needed_len {
            if ino != 0 {
                // Shrink current entry rec_len
                let new_actual_rec = actual_len as u16;
                dir_buf[offset + 4..offset + 6].copy_from_slice(&new_actual_rec.to_le_bytes());
                offset += actual_len;
            }

            let new_rec_len = (rec_len - actual_len) as u16;
            dir_buf[offset..offset + 4].copy_from_slice(&child_ino.to_le_bytes());
            dir_buf[offset + 4..offset + 6].copy_from_slice(&new_rec_len.to_le_bytes());
            dir_buf[offset + 6] = name_bytes.len() as u8;
            dir_buf[offset + 7] = file_type;
            dir_buf[offset + 8..offset + 8 + name_bytes.len()].copy_from_slice(name_bytes);

            inserted = true;
            break;
        }

        offset += rec_len;
    }

    if !inserted {
        // Append a new block
        let old_size = size;
        let block_size = volume.sb.block_size as usize;
        let new_size = old_size + block_size;
        dir_buf.resize(new_size, 0);

        let new_offset = old_size;
        let new_rec_len = block_size as u16;
        dir_buf[new_offset..new_offset + 4].copy_from_slice(&child_ino.to_le_bytes());
        dir_buf[new_offset + 4..new_offset + 6].copy_from_slice(&new_rec_len.to_le_bytes());
        dir_buf[new_offset + 6] = name_bytes.len() as u8;
        dir_buf[new_offset + 7] = file_type;
        dir_buf[new_offset + 8..new_offset + 8 + name_bytes.len()].copy_from_slice(name_bytes);

        dir_inode.size = new_size as u32;
    }

    volume.write_inode_data(dir_inode, dir_ino, 0, &dir_buf)?;
    Ok(())
}

/// Remove a directory entry by name from a directory inode. Returns the child inode number.
pub fn ext2_remove_entry(
    volume: &Arc<Ext2Volume>,
    dir_inode: &mut Ext2Inode,
    dir_ino: u32,
    name: &str,
) -> Result<u32, VfsError> {
    let size = dir_inode.size as usize;
    let mut dir_buf = alloc::vec![0u8; size];
    volume.read_inode_data(dir_inode, 0, &mut dir_buf)?;

    let mut offset = 0;
    let mut prev_offset: Option<usize> = None;

    while offset < size {
        if offset + 8 > size {
            break;
        }

        let ino = u32::from_le_bytes([
            dir_buf[offset],
            dir_buf[offset + 1],
            dir_buf[offset + 2],
            dir_buf[offset + 3],
        ]);
        let rec_len = u16::from_le_bytes([dir_buf[offset + 4], dir_buf[offset + 5]]) as usize;
        let name_len = dir_buf[offset + 6] as usize;

        if rec_len == 0 {
            break;
        }

        if ino != 0 && offset + 8 + name_len <= size {
            let name_bytes = &dir_buf[offset + 8..offset + 8 + name_len];
            if name_bytes == name.as_bytes() {
                if let Some(prev) = prev_offset {
                    let prev_rec = u16::from_le_bytes([dir_buf[prev + 4], dir_buf[prev + 5]]) as usize;
                    let combined = (prev_rec + rec_len) as u16;
                    dir_buf[prev + 4..prev + 6].copy_from_slice(&combined.to_le_bytes());
                } else {
                    dir_buf[offset..offset + 4].copy_from_slice(&0u32.to_le_bytes());
                }

                volume.write_inode_data(dir_inode, dir_ino, 0, &dir_buf)?;
                return Ok(ino);
            }
        }

        prev_offset = Some(offset);
        offset += rec_len;
    }

    Err(VfsError::NotFound)
}

