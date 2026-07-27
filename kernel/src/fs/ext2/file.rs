//! EXT2 File Operations (`file.rs`).

use alloc::sync::Arc;
use alloc::vec::Vec;
use ostd::Error;

use super::dir::read_directory_entries;
use super::inode::Ext2Inode;
use super::layout::{EXT2_S_IFDIR, Inode};
use super::superblock::{Ext2FsState, read_blocks, write_blocks};
use crate::fs::vfs::{DirEntry, FileOps, FileType, Result, SeekFrom};

pub fn get_or_alloc_block(
    fs_state: &Ext2FsState,
    inode: &mut Inode,
    block_index: u32,
    alloc: bool,
) -> Result<u32> {
    let block_size = fs_state.block_size;
    let ptrs_per_block = block_size / 4;

    if block_index < 12 {
        let block_id = inode.i_block[block_index as usize];
        if block_id == 0 && alloc {
            let new_block = fs_state.alloc_block()?;
            inode.i_block[block_index as usize] = new_block;
            inode.i_blocks += block_size / 512;
            Ok(new_block)
        } else {
            Ok(block_id)
        }
    } else if block_index < 12 + ptrs_per_block {
        let indirect_idx = block_index - 12;
        let mut indirect_block = inode.i_block[12];
        if indirect_block == 0 {
            if !alloc {
                return Ok(0);
            }
            indirect_block = fs_state.alloc_block()?;
            inode.i_block[12] = indirect_block;
            inode.i_blocks += block_size / 512;
        }

        let mut buf = alloc::vec![0u8; block_size as usize];
        read_blocks(&*fs_state.block_dev, block_size, indirect_block, &mut buf)?;

        let offset = (indirect_idx * 4) as usize;
        let mut block_id = u32::from_le_bytes([
            buf[offset],
            buf[offset + 1],
            buf[offset + 2],
            buf[offset + 3],
        ]);

        if block_id == 0 && alloc {
            block_id = fs_state.alloc_block()?;
            buf[offset..offset + 4].copy_from_slice(&block_id.to_le_bytes());
            write_blocks(&*fs_state.block_dev, block_size, indirect_block, &buf)?;
            inode.i_blocks += block_size / 512;
        }
        Ok(block_id)
    } else {
        Err(Error::NotEnoughResources)
    }
}

pub fn read_file_data(
    fs_state: &Ext2FsState,
    inode: &mut Inode,
    _inode_num: u32,
    offset: u64,
    buf: &mut [u8],
) -> Result<usize> {
    if offset >= inode.i_size as u64 {
        return Ok(0);
    }

    let block_size = fs_state.block_size as u64;
    let max_read = (inode.i_size as u64 - offset) as usize;
    let read_len = core::cmp::min(buf.len(), max_read);

    let mut bytes_read = 0;
    while bytes_read < read_len {
        let curr_offset = offset + bytes_read as u64;
        let block_index = (curr_offset / block_size) as u32;
        let offset_in_block = (curr_offset % block_size) as usize;
        let chunk_len =
            core::cmp::min(read_len - bytes_read, block_size as usize - offset_in_block);

        let block_id = get_or_alloc_block(fs_state, inode, block_index, false)?;
        if block_id == 0 {
            buf[bytes_read..bytes_read + chunk_len].fill(0);
        } else {
            let mut block_buf = alloc::vec![0u8; block_size as usize];
            read_blocks(
                &*fs_state.block_dev,
                fs_state.block_size,
                block_id,
                &mut block_buf,
            )?;
            buf[bytes_read..bytes_read + chunk_len]
                .copy_from_slice(&block_buf[offset_in_block..offset_in_block + chunk_len]);
        }
        bytes_read += chunk_len;
    }
    Ok(bytes_read)
}

pub fn write_file_data(
    fs_state: &Ext2FsState,
    inode: &mut Inode,
    inode_num: u32,
    offset: u64,
    buf: &[u8],
) -> Result<usize> {
    let block_size = fs_state.block_size as u64;
    let mut bytes_written = 0;

    while bytes_written < buf.len() {
        let curr_offset = offset + bytes_written as u64;
        let block_index = (curr_offset / block_size) as u32;
        let offset_in_block = (curr_offset % block_size) as usize;
        let chunk_len = core::cmp::min(
            buf.len() - bytes_written,
            block_size as usize - offset_in_block,
        );

        let block_id = get_or_alloc_block(fs_state, inode, block_index, true)?;
        let mut block_buf = alloc::vec![0u8; block_size as usize];

        if chunk_len < block_size as usize && block_id != 0 {
            read_blocks(
                &*fs_state.block_dev,
                fs_state.block_size,
                block_id,
                &mut block_buf,
            )?;
        }

        block_buf[offset_in_block..offset_in_block + chunk_len]
            .copy_from_slice(&buf[bytes_written..bytes_written + chunk_len]);
        write_blocks(
            &*fs_state.block_dev,
            fs_state.block_size,
            block_id,
            &block_buf,
        )?;

        bytes_written += chunk_len;
    }

    let new_size = (offset + bytes_written as u64) as u32;
    if new_size > inode.i_size {
        inode.i_size = new_size;
    }
    fs_state.write_inode(inode_num, inode)?;

    Ok(bytes_written)
}

pub fn truncate_inode_blocks(
    fs_state: &Ext2FsState,
    inode: &mut Inode,
    inode_num: u32,
) -> Result<()> {
    let block_size = fs_state.block_size;
    let ptrs_per_block = block_size / 4;

    for i in 0..12 {
        if inode.i_block[i] != 0 {
            fs_state.free_block(inode.i_block[i])?;
            inode.i_block[i] = 0;
        }
    }

    if inode.i_block[12] != 0 {
        let mut buf = alloc::vec![0u8; block_size as usize];
        read_blocks(
            &*fs_state.block_dev,
            block_size,
            inode.i_block[12],
            &mut buf,
        )?;
        for i in 0..ptrs_per_block {
            let offset = (i * 4) as usize;
            let block_id = u32::from_le_bytes([
                buf[offset],
                buf[offset + 1],
                buf[offset + 2],
                buf[offset + 3],
            ]);
            if block_id != 0 {
                fs_state.free_block(block_id)?;
            }
        }
        fs_state.free_block(inode.i_block[12])?;
        inode.i_block[12] = 0;
    }

    inode.i_size = 0;
    inode.i_blocks = 0;
    fs_state.write_inode(inode_num, inode)?;
    Ok(())
}

pub struct Ext2File {
    pub inode: Arc<Ext2Inode>,
}

impl FileOps for Ext2File {
    fn read(&mut self, buf: &mut [u8], offset: &mut usize) -> Result<usize> {
        let mut guard = self.inode.inode.lock();
        let bytes_read = read_file_data(
            &self.inode.fs,
            &mut guard,
            self.inode.inode_num,
            *offset as u64,
            buf,
        )?;
        *offset += bytes_read;
        Ok(bytes_read)
    }

    fn write(&mut self, buf: &[u8], offset: &mut usize) -> Result<usize> {
        let mut guard = self.inode.inode.lock();
        let bytes_written = write_file_data(
            &self.inode.fs,
            &mut guard,
            self.inode.inode_num,
            *offset as u64,
            buf,
        )?;
        *offset += bytes_written;
        Ok(bytes_written)
    }

    fn seek(&mut self, pos: SeekFrom, offset: &mut usize) -> Result<usize> {
        let guard = self.inode.inode.lock();
        let new_offset = match pos {
            SeekFrom::Start(val) => val as isize,
            SeekFrom::Current(val) => *offset as isize + val,
            SeekFrom::End(val) => guard.i_size as isize + val,
        };
        if new_offset < 0 {
            return Err(Error::InvalidArgs);
        }
        *offset = new_offset as usize;
        Ok(*offset)
    }

    fn readdir(&mut self) -> Result<Vec<DirEntry>> {
        let guard = self.inode.inode.lock();
        let is_dir = (guard.i_mode & EXT2_S_IFDIR) != 0;
        if !is_dir {
            return Err(Error::InvalidArgs);
        }

        let mut result = Vec::new();
        let entries = read_directory_entries(&self.inode.fs, self.inode.inode_num)?;
        for entry in entries {
            result.push(DirEntry {
                name: entry.name,
                inode_num: entry.inode_num as u64,
                file_type: if entry.is_dir {
                    FileType::Directory
                } else {
                    FileType::Regular
                },
            });
        }
        Ok(result)
    }
}
