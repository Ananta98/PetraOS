//! exFAT File Data I/O & FileOps Implementation (`file.rs`).

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use ostd::Error;

use super::dir::{read_directory_entries, write_dir_entry_set};
use super::fat::{read_bytes, write_bytes};
use super::inode::ExFatInode;
use super::superblock::ExFatFsState;
use crate::fs::vfs::{DirEntry, FileOps, FileType, Result, SeekFrom};

pub fn read_file_data(
    fs_state: &ExFatFsState,
    first_cluster: u32,
    no_fat_chain: bool,
    size: u64,
    offset: u64,
    buf: &mut [u8],
) -> Result<usize> {
    if offset >= size {
        return Ok(0);
    }
    let chain = fs_state.get_cluster_chain(first_cluster, no_fat_chain, size)?;
    let sector_size = 1u64 << fs_state.boot_sector.bytes_per_sector_shift;
    let cluster_size = sector_size * (1u64 << fs_state.boot_sector.sectors_per_cluster_shift);

    let max_read = (size - offset) as usize;
    let read_len = core::cmp::min(buf.len(), max_read);

    let mut bytes_read = 0;
    while bytes_read < read_len {
        let curr_offset = offset + bytes_read as u64;
        let cluster_idx = (curr_offset / cluster_size) as usize;
        let offset_in_cluster = (curr_offset % cluster_size) as usize;
        let chunk_len = core::cmp::min(
            read_len - bytes_read,
            cluster_size as usize - offset_in_cluster,
        );

        if cluster_idx >= chain.len() {
            break;
        }

        let cluster = chain[cluster_idx];
        let sector = fs_state.cluster_to_sector(cluster);
        let byte_offset = sector * sector_size + offset_in_cluster as u64;

        read_bytes(
            &*fs_state.block_dev,
            byte_offset,
            &mut buf[bytes_read..bytes_read + chunk_len],
        )?;
        bytes_read += chunk_len;
    }
    Ok(bytes_read)
}

pub fn write_file_data(
    fs_state: &ExFatFsState,
    first_cluster: u32,
    no_fat_chain: bool,
    size: u64,
    offset: u64,
    buf: &[u8],
) -> Result<usize> {
    let chain = fs_state.get_cluster_chain(first_cluster, no_fat_chain, size)?;
    let sector_size = 1u64 << fs_state.boot_sector.bytes_per_sector_shift;
    let cluster_size = sector_size * (1u64 << fs_state.boot_sector.sectors_per_cluster_shift);

    let mut bytes_written = 0;
    while bytes_written < buf.len() {
        let curr_offset = offset + bytes_written as u64;
        let cluster_idx = (curr_offset / cluster_size) as usize;
        let offset_in_cluster = (curr_offset % cluster_size) as usize;
        let chunk_len = core::cmp::min(
            buf.len() - bytes_written,
            cluster_size as usize - offset_in_cluster,
        );

        if cluster_idx >= chain.len() {
            break;
        }

        let cluster = chain[cluster_idx];
        let sector = fs_state.cluster_to_sector(cluster);
        let byte_offset = sector * sector_size + offset_in_cluster as u64;

        write_bytes(
            &*fs_state.block_dev,
            byte_offset,
            &buf[bytes_written..bytes_written + chunk_len],
        )?;
        bytes_written += chunk_len;
    }
    Ok(bytes_written)
}

pub fn extend_file(
    fs_state: &ExFatFsState,
    first_cluster: &mut u32,
    no_fat_chain: &mut bool,
    size: &mut u64,
    new_size: u64,
    parent_cluster: u32,
    parent_no_fat: bool,
    parent_size: u64,
    entry_offset: usize,
) -> Result<()> {
    let sector_size = 1u64 << fs_state.boot_sector.bytes_per_sector_shift;
    let cluster_size = sector_size * (1u64 << fs_state.boot_sector.sectors_per_cluster_shift);

    let curr_clusters = (*size + cluster_size - 1) / cluster_size;
    let needed_clusters = (new_size + cluster_size - 1) / cluster_size;

    if needed_clusters > curr_clusters {
        let additional = (needed_clusters - curr_clusters) as usize;
        let mut new_chain = Vec::new();
        for _ in 0..additional {
            let allocated = fs_state.alloc_cluster()?;
            new_chain.push(allocated);
        }

        if *first_cluster == 0 {
            *first_cluster = new_chain[0];
            *no_fat_chain = true;
            for i in 0..new_chain.len() - 1 {
                if new_chain[i + 1] != new_chain[i] + 1 {
                    *no_fat_chain = false;
                    break;
                }
            }
            if !*no_fat_chain {
                for i in 0..new_chain.len() - 1 {
                    super::fat::set_next_cluster(
                        &*fs_state.block_dev,
                        &fs_state.boot_sector,
                        new_chain[i],
                        new_chain[i + 1],
                    )?;
                }
            }
        } else {
            let existing_chain =
                fs_state.get_cluster_chain(*first_cluster, *no_fat_chain, *size)?;
            let last_cluster = *existing_chain.last().unwrap();

            if *no_fat_chain && new_chain[0] == last_cluster + 1 {
                let mut contiguous = true;
                for i in 0..new_chain.len() - 1 {
                    if new_chain[i + 1] != new_chain[i] + 1 {
                        contiguous = false;
                        break;
                    }
                }
                if !contiguous {
                    *no_fat_chain = false;
                    for i in 0..existing_chain.len() - 1 {
                        super::fat::set_next_cluster(
                            &*fs_state.block_dev,
                            &fs_state.boot_sector,
                            existing_chain[i],
                            existing_chain[i + 1],
                        )?;
                    }
                    super::fat::set_next_cluster(
                        &*fs_state.block_dev,
                        &fs_state.boot_sector,
                        last_cluster,
                        new_chain[0],
                    )?;
                    for i in 0..new_chain.len() - 1 {
                        super::fat::set_next_cluster(
                            &*fs_state.block_dev,
                            &fs_state.boot_sector,
                            new_chain[i],
                            new_chain[i + 1],
                        )?;
                    }
                }
            } else {
                if *no_fat_chain {
                    *no_fat_chain = false;
                    for i in 0..existing_chain.len() - 1 {
                        super::fat::set_next_cluster(
                            &*fs_state.block_dev,
                            &fs_state.boot_sector,
                            existing_chain[i],
                            existing_chain[i + 1],
                        )?;
                    }
                }
                super::fat::set_next_cluster(
                    &*fs_state.block_dev,
                    &fs_state.boot_sector,
                    last_cluster,
                    new_chain[0],
                )?;
                for i in 0..new_chain.len() - 1 {
                    super::fat::set_next_cluster(
                        &*fs_state.block_dev,
                        &fs_state.boot_sector,
                        new_chain[i],
                        new_chain[i + 1],
                    )?;
                }
            }
        }
    }

    *size = new_size;

    if parent_cluster != 0 {
        let mut entry_buf = [0u8; 32];
        read_file_data(
            fs_state,
            parent_cluster,
            parent_no_fat,
            parent_size,
            (entry_offset + 32) as u64,
            &mut entry_buf,
        )?;
        let flags = if *no_fat_chain { 0x03 } else { 0x01 };
        entry_buf[1] = flags;
        entry_buf[20..24].copy_from_slice(&first_cluster.to_le_bytes());
        entry_buf[24..32].copy_from_slice(&size.to_le_bytes());

        write_file_data(
            fs_state,
            parent_cluster,
            parent_no_fat,
            parent_size,
            (entry_offset + 32) as u64,
            &entry_buf,
        )?;
    }
    Ok(())
}

pub struct ExFatFile {
    pub inode: Arc<ExFatInode>,
}

impl FileOps for ExFatFile {
    fn read(&mut self, buf: &mut [u8], offset: &mut usize) -> Result<usize> {
        let info = self.inode.file_info.lock();
        if info.is_dir {
            return Err(Error::InvalidArgs);
        }
        let bytes_read = read_file_data(
            &self.inode.fs,
            info.first_cluster,
            info.no_fat_chain,
            info.size,
            *offset as u64,
            buf,
        )?;
        *offset += bytes_read;
        Ok(bytes_read)
    }

    fn write(&mut self, buf: &[u8], offset: &mut usize) -> Result<usize> {
        let mut info = self.inode.file_info.lock();
        if info.is_dir {
            return Err(Error::InvalidArgs);
        }
        let write_end = *offset + buf.len();
        if write_end as u64 > info.size {
            let parent_cluster = info.entry_cluster;
            let (parent_no_fat, parent_size) =
                if parent_cluster == self.inode.fs.boot_sector.first_cluster_of_root {
                    let root = self.inode.fs.root_info.lock();
                    (root.no_fat_chain, root.size)
                } else {
                    let root = self.inode.fs.root_info.lock();
                    (root.no_fat_chain, root.size)
                };

            let mut first_cluster = info.first_cluster;
            let mut no_fat_chain = info.no_fat_chain;
            let mut size = info.size;

            extend_file(
                &self.inode.fs,
                &mut first_cluster,
                &mut no_fat_chain,
                &mut size,
                write_end as u64,
                parent_cluster,
                parent_no_fat,
                parent_size,
                info.entry_offset_in_dir,
            )?;

            info.first_cluster = first_cluster;
            info.no_fat_chain = no_fat_chain;
            info.size = size;
        }

        let bytes_written = write_file_data(
            &self.inode.fs,
            info.first_cluster,
            info.no_fat_chain,
            info.size,
            *offset as u64,
            buf,
        )?;
        *offset += bytes_written;
        Ok(bytes_written)
    }

    fn seek(&mut self, pos: SeekFrom, offset: &mut usize) -> Result<usize> {
        let info = self.inode.file_info.lock();
        let new_offset = match pos {
            SeekFrom::Start(val) => val as isize,
            SeekFrom::Current(val) => *offset as isize + val,
            SeekFrom::End(val) => info.size as isize + val,
        };
        if new_offset < 0 {
            return Err(Error::InvalidArgs);
        }
        *offset = new_offset as usize;
        Ok(*offset)
    }

    fn readdir(&mut self) -> Result<Vec<DirEntry>> {
        let info = self.inode.file_info.lock();
        if !info.is_dir {
            return Err(Error::InvalidArgs);
        }
        let mut result = Vec::new();
        result.push(DirEntry {
            name: String::from("."),
            inode_num: info.first_cluster as u64,
            file_type: FileType::Directory,
        });
        result.push(DirEntry {
            name: String::from(".."),
            inode_num: 0,
            file_type: FileType::Directory,
        });

        let files = read_directory_entries(
            &self.inode.fs,
            info.first_cluster,
            info.no_fat_chain,
            info.size,
        )?;
        for file in files {
            let inode_num = if file.first_cluster != 0 {
                file.first_cluster as u64
            } else {
                ((file.entry_cluster as u64) << 32) | (file.entry_offset_in_dir as u64)
            };
            result.push(DirEntry {
                name: file.name,
                inode_num,
                file_type: if file.is_dir {
                    FileType::Directory
                } else {
                    FileType::Regular
                },
            });
        }
        Ok(result)
    }
}
