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
