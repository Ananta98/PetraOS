//! EXT2 Directory Operations (`dir.rs`).

use alloc::string::String;
use alloc::vec::Vec;
use ostd::Error;

use crate::fs::vfs::Result;
use super::file::read_file_data;
use super::layout::{EXT2_FT_DIR, EXT2_FT_REG_FILE, Ext2FileInfo, Inode};
use super::superblock::{Ext2FsState, write_blocks};

pub fn read_directory_entries(
    fs_state: &Ext2FsState,
    dir_inode_num: u32,
) -> Result<Vec<Ext2FileInfo>> {
    let mut dir_inode = fs_state.read_inode(dir_inode_num)?;
    let mut buf = alloc::vec![0u8; dir_inode.i_size as usize];
    let bytes_read = read_file_data(fs_state, &mut dir_inode, dir_inode_num, 0, &mut buf)?;
    buf.truncate(bytes_read);

    let mut entries = Vec::new();
    let mut offset = 0;

    while offset < buf.len() {
        if offset + 8 > buf.len() {
            break;
        }

        let inode_num =
            u32::from_le_bytes([buf[offset], buf[offset + 1], buf[offset + 2], buf[offset + 3]]);
        let rec_len = u16::from_le_bytes([buf[offset + 4], buf[offset + 5]]) as usize;
        let name_len = buf[offset + 6] as usize;
        let file_type = buf[offset + 7];

        if rec_len == 0 {
            break;
        }

        if inode_num != 0 && offset + 8 + name_len <= buf.len() {
            let name_bytes = &buf[offset + 8..offset + 8 + name_len];
            if let Ok(name) = core::str::from_utf8(name_bytes) {
                let is_dir = file_type == EXT2_FT_DIR;
                entries.push(Ext2FileInfo {
                    name: String::from(name),
                    inode_num,
                    mode: 0,
                    size: 0,
                    is_dir,
                });
            }
        }

        offset += rec_len;
    }

    Ok(entries)
}

pub fn add_directory_entry(
    fs_state: &Ext2FsState,
    dir_inode_num: u32,
    name: &str,
    new_inode_num: u32,
    is_dir: bool,
) -> Result<()> {
    let mut dir_inode = fs_state.read_inode(dir_inode_num)?;
    let block_size = fs_state.block_size as usize;
    let name_bytes = name.as_bytes();
    let required_len = (8 + name_bytes.len() + 3) & !3; // 4-byte aligned

    let mut block_idx = 0;
    while block_idx < 12 {
        let block_id = dir_inode.i_block[block_idx];
        if block_id == 0 {
            break;
        }

        let mut block_buf = alloc::vec![0u8; block_size];
        super::superblock::read_blocks(&*fs_state.block_dev, fs_state.block_size, block_id, &mut block_buf)?;

        let mut offset = 0;
        while offset < block_size {
            let rec_len =
                u16::from_le_bytes([block_buf[offset + 4], block_buf[offset + 5]]) as usize;
            let name_len = block_buf[offset + 6] as usize;
            let actual_len = (8 + name_len + 3) & !3;
            let available_space = rec_len.saturating_sub(actual_len);

            if available_space >= required_len {
                // Split entry
                block_buf[offset + 4..offset + 6]
                    .copy_from_slice(&(actual_len as u16).to_le_bytes());

                let new_offset = offset + actual_len;
                let new_rec_len = rec_len - actual_len;

                block_buf[new_offset..new_offset + 4]
                    .copy_from_slice(&new_inode_num.to_le_bytes());
                block_buf[new_offset + 4..new_offset + 6]
                    .copy_from_slice(&(new_rec_len as u16).to_le_bytes());
                block_buf[new_offset + 6] = name_bytes.len() as u8;
                block_buf[new_offset + 7] = if is_dir {
                    EXT2_FT_DIR
                } else {
                    EXT2_FT_REG_FILE
                };
                block_buf[new_offset + 8..new_offset + 8 + name_bytes.len()]
                    .copy_from_slice(name_bytes);

                write_blocks(&*fs_state.block_dev, fs_state.block_size, block_id, &block_buf)?;
                return Ok(());
            }

            offset += rec_len;
        }

        block_idx += 1;
    }

    // Allocate new block for directory
    let new_block_id = fs_state.alloc_block()?;
    if block_idx < 12 {
        dir_inode.i_block[block_idx] = new_block_id;
        dir_inode.i_size += fs_state.block_size;
        dir_inode.i_blocks += fs_state.block_size / 512;
        fs_state.write_inode(dir_inode_num, &dir_inode)?;
    } else {
        return Err(Error::NotEnoughResources);
    }

    let mut block_buf = alloc::vec![0u8; block_size];
    block_buf[0..4].copy_from_slice(&new_inode_num.to_le_bytes());
    block_buf[4..6].copy_from_slice(&(block_size as u16).to_le_bytes());
    block_buf[6] = name_bytes.len() as u8;
    block_buf[7] = if is_dir {
        EXT2_FT_DIR
    } else {
        EXT2_FT_REG_FILE
    };
    block_buf[8..8 + name_bytes.len()].copy_from_slice(name_bytes);

    write_blocks(&*fs_state.block_dev, fs_state.block_size, new_block_id, &block_buf)?;
    Ok(())
}

pub fn remove_directory_entry(
    fs_state: &Ext2FsState,
    dir_inode_num: u32,
    name: &str,
) -> Result<u32> {
    let dir_inode = fs_state.read_inode(dir_inode_num)?;
    let block_size = fs_state.block_size as usize;

    for block_idx in 0..12 {
        let block_id = dir_inode.i_block[block_idx];
        if block_id == 0 {
            break;
        }

        let mut block_buf = alloc::vec![0u8; block_size];
        super::superblock::read_blocks(&*fs_state.block_dev, fs_state.block_size, block_id, &mut block_buf)?;

        let mut offset = 0;
        let mut prev_offset = None;

        while offset < block_size {
            let inode_num = u32::from_le_bytes([
                block_buf[offset],
                block_buf[offset + 1],
                block_buf[offset + 2],
                block_buf[offset + 3],
            ]);
            let rec_len =
                u16::from_le_bytes([block_buf[offset + 4], block_buf[offset + 5]]) as usize;
            let name_len = block_buf[offset + 6] as usize;

            if rec_len == 0 {
                break;
            }

            if inode_num != 0 && offset + 8 + name_len <= block_size {
                let name_bytes = &block_buf[offset + 8..offset + 8 + name_len];
                if let Ok(entry_name) = core::str::from_utf8(name_bytes) {
                    if entry_name == name {
                        if let Some(prev) = prev_offset {
                            let prev_rec_len = u16::from_le_bytes([
                                block_buf[prev + 4],
                                block_buf[prev + 5],
                            ]) as usize;
                            let new_prev_rec_len = prev_rec_len + rec_len;
                            block_buf[prev + 4..prev + 6]
                                .copy_from_slice(&(new_prev_rec_len as u16).to_le_bytes());
                        } else {
                            block_buf[offset..offset + 4].copy_from_slice(&0u32.to_le_bytes());
                        }

                        write_blocks(
                            &*fs_state.block_dev,
                            fs_state.block_size,
                            block_id,
                            &block_buf,
                        )?;
                        return Ok(inode_num);
                    }
                }
            }

            prev_offset = Some(offset);
            offset += rec_len;
        }
    }

    Err(Error::InvalidArgs)
}
