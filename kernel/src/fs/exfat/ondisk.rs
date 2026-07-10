use crate::drivers::block::BlockDevice;
use crate::fs::vfs::Result;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use ostd::Error;
use ostd::sync::SpinLock;

use super::structs::{BootSector, ExFatFileInfo};

pub fn read_bytes(block_dev: &dyn BlockDevice, offset: u64, buf: &mut [u8]) -> Result<()> {
    let mut block_buf = [0u8; 512];
    let mut bytes_read = 0;
    while bytes_read < buf.len() {
        let curr_offset = offset + bytes_read as u64;
        let block_id = (curr_offset / 512) as usize;
        let block_offset = (curr_offset % 512) as usize;
        let chunk_len = core::cmp::min(buf.len() - bytes_read, 512 - block_offset);

        block_dev.read_blocks(block_id, &mut block_buf)?;
        buf[bytes_read..bytes_read + chunk_len]
            .copy_from_slice(&block_buf[block_offset..block_offset + chunk_len]);
        bytes_read += chunk_len;
    }
    Ok(())
}

pub fn write_bytes(block_dev: &dyn BlockDevice, offset: u64, buf: &[u8]) -> Result<()> {
    let mut block_buf = [0u8; 512];
    let mut bytes_written = 0;
    while bytes_written < buf.len() {
        let curr_offset = offset + bytes_written as u64;
        let block_id = (curr_offset / 512) as usize;
        let block_offset = (curr_offset % 512) as usize;
        let chunk_len = core::cmp::min(buf.len() - bytes_written, 512 - block_offset);

        if chunk_len < 512 {
            block_dev.read_blocks(block_id, &mut block_buf)?;
        }
        block_buf[block_offset..block_offset + chunk_len]
            .copy_from_slice(&buf[bytes_written..bytes_written + chunk_len]);
        block_dev.write_blocks(block_id, &block_buf)?;
        bytes_written += chunk_len;
    }
    Ok(())
}

pub struct ExFatFsState {
    pub block_dev: Arc<dyn BlockDevice>,
    pub boot_sector: BootSector,
    pub bitmap_first_cluster: AtomicU32,
    pub bitmap_size: AtomicU64,
    pub root_info: SpinLock<ExFatFileInfo>,
}

impl ExFatFsState {
    pub fn cluster_to_sector(&self, cluster: u32) -> u64 {
        let cluster_heap_offset_sectors = self.boot_sector.cluster_heap_offset as u64;
        let sectors_per_cluster = 1u64 << self.boot_sector.sectors_per_cluster_shift;
        cluster_heap_offset_sectors + (cluster as u64 - 2) * sectors_per_cluster
    }

    pub fn get_next_cluster(&self, cluster: u32) -> Result<u32> {
        let sector_size = 1u64 << self.boot_sector.bytes_per_sector_shift;
        let fat_offset_bytes = (self.boot_sector.fat_offset as u64) * sector_size;
        let entry_offset = fat_offset_bytes + (cluster as u64) * 4;
        let mut buf = [0u8; 4];
        read_bytes(&*self.block_dev, entry_offset, &mut buf)?;
        Ok(u32::from_le_bytes(buf))
    }

    pub fn set_next_cluster(&self, cluster: u32, next: u32) -> Result<()> {
        let sector_size = 1u64 << self.boot_sector.bytes_per_sector_shift;
        let fat_offset_bytes = (self.boot_sector.fat_offset as u64) * sector_size;
        let entry_offset = fat_offset_bytes + (cluster as u64) * 4;
        let buf = next.to_le_bytes();
        write_bytes(&*self.block_dev, entry_offset, &buf)?;
        Ok(())
    }

    pub fn get_cluster_chain(
        &self,
        first_cluster: u32,
        no_fat_chain: bool,
        size: u64,
    ) -> Result<Vec<u32>> {
        let mut chain = Vec::new();
        if size == 0 || first_cluster == 0 {
            return Ok(chain);
        }
        let sector_size = 1u64 << self.boot_sector.bytes_per_sector_shift;
        let cluster_size = sector_size * (1u64 << self.boot_sector.sectors_per_cluster_shift);
        let num_clusters = (size + cluster_size - 1) / cluster_size;

        if no_fat_chain {
            for i in 0..num_clusters {
                chain.push(first_cluster + i as u32);
            }
        } else {
            let mut curr = first_cluster;
            while curr >= 2 && curr <= 0xFFFFFFF6 {
                chain.push(curr);
                curr = self.get_next_cluster(curr)?;
            }
        }
        Ok(chain)
    }

    pub fn read_file_data(
        &self,
        first_cluster: u32,
        no_fat_chain: bool,
        size: u64,
        offset: u64,
        buf: &mut [u8],
    ) -> Result<usize> {
        if offset >= size {
            return Ok(0);
        }
        let read_len = core::cmp::min(buf.len() as u64, size - offset) as usize;
        let sector_size = 1u64 << self.boot_sector.bytes_per_sector_shift;
        let cluster_size = sector_size * (1u64 << self.boot_sector.sectors_per_cluster_shift);
        let chain = self.get_cluster_chain(first_cluster, no_fat_chain, size)?;

        let mut bytes_read = 0;
        while bytes_read < read_len {
            let curr_offset = offset + bytes_read as u64;
            let chain_idx = (curr_offset / cluster_size) as usize;
            if chain_idx >= chain.len() {
                break;
            }
            let cluster = chain[chain_idx];
            let cluster_offset = (curr_offset % cluster_size) as u64;
            let sector_offset = self.cluster_to_sector(cluster) * sector_size;
            let chunk = core::cmp::min(
                read_len - bytes_read,
                (cluster_size - cluster_offset) as usize,
            );

            read_bytes(
                &*self.block_dev,
                sector_offset + cluster_offset,
                &mut buf[bytes_read..bytes_read + chunk],
            )?;
            bytes_read += chunk;
        }
        Ok(bytes_read)
    }

    pub fn write_file_data(
        &self,
        first_cluster: u32,
        no_fat_chain: bool,
        size: u64,
        offset: u64,
        buf: &[u8],
    ) -> Result<usize> {
        let sector_size = 1u64 << self.boot_sector.bytes_per_sector_shift;
        let cluster_size = sector_size * (1u64 << self.boot_sector.sectors_per_cluster_shift);
        let chain = self.get_cluster_chain(first_cluster, no_fat_chain, size)?;

        let mut bytes_written = 0;
        while bytes_written < buf.len() {
            let curr_offset = offset + bytes_written as u64;
            let chain_idx = (curr_offset / cluster_size) as usize;
            if chain_idx >= chain.len() {
                break;
            }
            let cluster = chain[chain_idx];
            let cluster_offset = (curr_offset % cluster_size) as u64;
            let sector_offset = self.cluster_to_sector(cluster) * sector_size;
            let chunk = core::cmp::min(
                buf.len() - bytes_written,
                (cluster_size - cluster_offset) as usize,
            );

            write_bytes(
                &*self.block_dev,
                sector_offset + cluster_offset,
                &buf[bytes_written..bytes_written + chunk],
            )?;
            bytes_written += chunk;
        }
        Ok(bytes_written)
    }

    pub fn set_bitmap_bit(&self, cluster: u32, val: bool) -> Result<()> {
        let bitmap_cluster = self.bitmap_first_cluster.load(Ordering::Relaxed);
        if bitmap_cluster == 0 {
            return Ok(());
        }
        let cluster_idx = cluster - 2;
        let byte_offset = (cluster_idx / 8) as u64;
        let bit_offset = cluster_idx % 8;

        let sector_size = 1u64 << self.boot_sector.bytes_per_sector_shift;
        let bitmap_sector = self.cluster_to_sector(bitmap_cluster);
        let byte_addr = bitmap_sector * sector_size + byte_offset;

        let mut byte = [0u8; 1];
        read_bytes(&*self.block_dev, byte_addr, &mut byte)?;
        if val {
            byte[0] |= 1 << bit_offset;
        } else {
            byte[0] &= !(1 << bit_offset);
        }
        write_bytes(&*self.block_dev, byte_addr, &byte)?;
        Ok(())
    }

    pub fn alloc_cluster(&self) -> Result<u32> {
        let cluster_count = self.boot_sector.cluster_count;
        for cluster in 2..cluster_count + 2 {
            let val = self.get_next_cluster(cluster)?;
            if val == 0 {
                self.set_next_cluster(cluster, 0xFFFFFFFF)?;
                self.set_bitmap_bit(cluster, true)?;
                return Ok(cluster);
            }
        }
        Err(Error::IoError)
    }

    pub fn free_cluster_chain(
        &self,
        first_cluster: u32,
        no_fat_chain: bool,
        size: u64,
    ) -> Result<()> {
        let chain = self.get_cluster_chain(first_cluster, no_fat_chain, size)?;
        for cluster in chain {
            self.set_next_cluster(cluster, 0)?;
            self.set_bitmap_bit(cluster, false)?;
        }
        Ok(())
    }

    pub fn read_directory_entries(
        &self,
        dir_cluster: u32,
        dir_no_fat: bool,
        dir_size: u64,
    ) -> Result<Vec<ExFatFileInfo>> {
        let mut files = Vec::new();
        let mut offset = 0;
        let mut buf = [0u8; 32];

        while offset < dir_size {
            let bytes_read =
                self.read_file_data(dir_cluster, dir_no_fat, dir_size, offset, &mut buf)?;
            if bytes_read < 32 {
                break;
            }

            let entry_type = buf[0];
            if entry_type == 0 {
                break; // End of directory
            }

            if entry_type == 0x85 {
                let file_entry_offset = offset;
                let file_attr = u16::from_le_bytes([buf[4], buf[5]]);
                let secondary_count = buf[1];

                offset += 32;
                let mut stream_buf = [0u8; 32];
                self.read_file_data(dir_cluster, dir_no_fat, dir_size, offset, &mut stream_buf)?;
                if stream_buf[0] != 0xC0 {
                    continue;
                }

                let flags = stream_buf[1];
                let name_length = stream_buf[3] as usize;
                let first_cluster = u32::from_le_bytes([
                    stream_buf[20],
                    stream_buf[21],
                    stream_buf[22],
                    stream_buf[23],
                ]);
                let data_length = u64::from_le_bytes([
                    stream_buf[24],
                    stream_buf[25],
                    stream_buf[26],
                    stream_buf[27],
                    stream_buf[28],
                    stream_buf[29],
                    stream_buf[30],
                    stream_buf[31],
                ]);
                let no_fat_chain = (flags & 0x02) != 0;

                let mut name_utf16 = Vec::new();
                for _ in 0..secondary_count - 1 {
                    offset += 32;
                    let mut name_buf = [0u8; 32];
                    self.read_file_data(dir_cluster, dir_no_fat, dir_size, offset, &mut name_buf)?;
                    if name_buf[0] != 0xC1 {
                        continue;
                    }
                    for chunk in 0..15 {
                        let char_offset = 2 + chunk * 2;
                        let char_val =
                            u16::from_le_bytes([name_buf[char_offset], name_buf[char_offset + 1]]);
                        if char_val != 0 {
                            name_utf16.push(char_val);
                        }
                    }
                }

                let name = String::from_utf16_lossy(
                    &name_utf16[..core::cmp::min(name_utf16.len(), name_length)],
                );
                let is_dir = (file_attr & 0x10) != 0;

                files.push(ExFatFileInfo {
                    name,
                    file_attributes: file_attr,
                    first_cluster,
                    size: data_length,
                    is_dir,
                    no_fat_chain,
                    entry_cluster: dir_cluster,
                    entry_offset_in_dir: file_entry_offset as usize,
                    entry_count: (1 + secondary_count) as usize,
                });
            }
            offset += 32;
        }

        Ok(files)
    }

    pub fn update_file_size_in_dir(
        &self,
        parent_cluster: u32,
        parent_no_fat: bool,
        parent_size: u64,
        entry_offset: usize,
        new_size: u64,
    ) -> Result<()> {
        let stream_entry_offset = (entry_offset + 32) as u64;
        let mut stream_buf = [0u8; 32];
        self.read_file_data(
            parent_cluster,
            parent_no_fat,
            parent_size,
            stream_entry_offset,
            &mut stream_buf,
        )?;

        if stream_buf[0] != 0xC0 {
            return Err(Error::IoError);
        }

        stream_buf[8..16].copy_from_slice(&new_size.to_le_bytes());
        stream_buf[24..32].copy_from_slice(&new_size.to_le_bytes());

        self.write_file_data(
            parent_cluster,
            parent_no_fat,
            parent_size,
            stream_entry_offset,
            &stream_buf,
        )?;
        Ok(())
    }

    pub fn extend_file(
        &self,
        first_cluster: &mut u32,
        no_fat_chain: &mut bool,
        size: &mut u64,
        new_size: u64,
        parent_cluster: u32,
        parent_no_fat: bool,
        parent_size: u64,
        entry_offset: usize,
    ) -> Result<()> {
        let sector_size = 1u64 << self.boot_sector.bytes_per_sector_shift;
        let cluster_size = sector_size * (1u64 << self.boot_sector.sectors_per_cluster_shift);
        let old_num_clusters = (*size + cluster_size - 1) / cluster_size;
        let new_num_clusters = (new_size + cluster_size - 1) / cluster_size;

        if new_num_clusters > old_num_clusters {
            let mut chain = self.get_cluster_chain(*first_cluster, *no_fat_chain, *size)?;
            let num_to_alloc = new_num_clusters - old_num_clusters;

            for _ in 0..num_to_alloc {
                let new_cluster = self.alloc_cluster()?;
                if let Some(&last) = chain.last() {
                    self.set_next_cluster(last, new_cluster)?;
                } else {
                    *first_cluster = new_cluster;
                    let stream_entry_offset = (entry_offset + 32) as u64;
                    let mut stream_buf = [0u8; 32];
                    self.read_file_data(
                        parent_cluster,
                        parent_no_fat,
                        parent_size,
                        stream_entry_offset,
                        &mut stream_buf,
                    )?;
                    stream_buf[20..24].copy_from_slice(&new_cluster.to_le_bytes());
                    self.write_file_data(
                        parent_cluster,
                        parent_no_fat,
                        parent_size,
                        stream_entry_offset,
                        &stream_buf,
                    )?;
                }
                chain.push(new_cluster);
            }

            if *no_fat_chain && chain.len() > 1 {
                *no_fat_chain = false;
                let stream_entry_offset = (entry_offset + 32) as u64;
                let mut stream_buf = [0u8; 32];
                self.read_file_data(
                    parent_cluster,
                    parent_no_fat,
                    parent_size,
                    stream_entry_offset,
                    &mut stream_buf,
                )?;
                stream_buf[1] &= !0x02; // Clear NoFatChain bit
                self.write_file_data(
                    parent_cluster,
                    parent_no_fat,
                    parent_size,
                    stream_entry_offset,
                    &stream_buf,
                )?;
            }
        }

        *size = new_size;
        // Skip size update in parent entry if parent is 0 (signaling root directory which has no parent entry)
        if parent_cluster != 0 {
            self.update_file_size_in_dir(
                parent_cluster,
                parent_no_fat,
                parent_size,
                entry_offset,
                new_size,
            )?;
        }
        Ok(())
    }

    pub fn find_free_dir_slots(
        &self,
        dir_cluster: u32,
        dir_no_fat: &mut bool,
        dir_size: &mut u64,
        parent_parent_cluster: u32,
        parent_parent_no_fat: bool,
        parent_parent_size: u64,
        parent_entry_offset: usize,
        slots_needed: usize,
    ) -> Result<usize> {
        let mut offset = 0;
        let mut contiguous_slots = 0;
        let mut start_offset = 0;
        let mut buf = [0u8; 32];

        while offset < *dir_size {
            self.read_file_data(dir_cluster, *dir_no_fat, *dir_size, offset as u64, &mut buf)?;
            let entry_type = buf[0];

            if entry_type == 0 {
                let bytes_remaining = *dir_size - offset;
                let bytes_needed = (slots_needed * 32) as u64;
                if bytes_remaining < bytes_needed {
                    let sector_size = 1u64 << self.boot_sector.bytes_per_sector_shift;
                    let cluster_size =
                        sector_size * (1u64 << self.boot_sector.sectors_per_cluster_shift);
                    let mut temp_cluster = dir_cluster;
                    self.extend_file(
                        &mut temp_cluster,
                        dir_no_fat,
                        dir_size,
                        *dir_size + cluster_size,
                        parent_parent_cluster,
                        parent_parent_no_fat,
                        parent_parent_size,
                        parent_entry_offset,
                    )?;
                    let zeros = alloc::vec![0u8; cluster_size as usize];
                    self.write_file_data(
                        dir_cluster,
                        *dir_no_fat,
                        *dir_size,
                        offset as u64,
                        &zeros,
                    )?;
                }
                return Ok(offset as usize);
            }

            if (entry_type & 0x80) == 0 {
                if contiguous_slots == 0 {
                    start_offset = offset;
                }
                contiguous_slots += 1;
                if contiguous_slots == slots_needed {
                    return Ok(start_offset as usize);
                }
            } else {
                contiguous_slots = 0;
            }
            offset += 32;
        }

        let sector_size = 1u64 << self.boot_sector.bytes_per_sector_shift;
        let cluster_size = sector_size * (1u64 << self.boot_sector.sectors_per_cluster_shift);
        let mut temp_cluster = dir_cluster;
        let old_size = *dir_size;
        self.extend_file(
            &mut temp_cluster,
            dir_no_fat,
            dir_size,
            old_size + cluster_size,
            parent_parent_cluster,
            parent_parent_no_fat,
            parent_parent_size,
            parent_entry_offset,
        )?;
        let zeros = alloc::vec![0u8; cluster_size as usize];
        self.write_file_data(dir_cluster, *dir_no_fat, *dir_size, old_size, &zeros)?;
        Ok(old_size as usize)
    }

    pub fn write_dir_entry_set(
        &self,
        dir_cluster: u32,
        dir_no_fat: bool,
        dir_size: u64,
        offset: usize,
        name: &str,
        file_attr: u16,
        first_cluster: u32,
        size: u64,
    ) -> Result<()> {
        let name_utf16: Vec<u16> = name.encode_utf16().collect();
        let name_len = name_utf16.len();
        let name_entries = (name_len + 14) / 15;
        let secondary_count = 1 + name_entries;

        let mut file_buf = [0u8; 32];
        file_buf[0] = 0x85;
        file_buf[1] = secondary_count as u8;
        file_buf[4..6].copy_from_slice(&file_attr.to_le_bytes());
        self.write_file_data(dir_cluster, dir_no_fat, dir_size, offset as u64, &file_buf)?;

        let mut stream_buf = [0u8; 32];
        stream_buf[0] = 0xC0;
        stream_buf[1] = 0x03; // AllocationPossible | NoFatChain
        stream_buf[3] = name_len as u8;
        stream_buf[20..24].copy_from_slice(&first_cluster.to_le_bytes());
        stream_buf[24..32].copy_from_slice(&size.to_le_bytes());
        self.write_file_data(
            dir_cluster,
            dir_no_fat,
            dir_size,
            (offset + 32) as u64,
            &stream_buf,
        )?;

        for i in 0..name_entries {
            let mut name_buf = [0u8; 32];
            name_buf[0] = 0xC1;
            for j in 0..15 {
                let char_idx = i * 15 + j;
                let char_val = if char_idx < name_len {
                    name_utf16[char_idx]
                } else {
                    0
                };
                let char_offset = 2 + j * 2;
                name_buf[char_offset..char_offset + 2].copy_from_slice(&char_val.to_le_bytes());
            }
            self.write_file_data(
                dir_cluster,
                dir_no_fat,
                dir_size,
                (offset + 64 + i * 32) as u64,
                &name_buf,
            )?;
        }
        Ok(())
    }

    pub fn delete_dir_entry_set(
        &self,
        parent_cluster: u32,
        parent_no_fat: bool,
        parent_size: u64,
        entry_offset: usize,
        entry_count: usize,
    ) -> Result<()> {
        let mut buf = [0u8; 32];
        for i in 0..entry_count {
            let curr_offset = (entry_offset + i * 32) as u64;
            self.read_file_data(
                parent_cluster,
                parent_no_fat,
                parent_size,
                curr_offset,
                &mut buf,
            )?;
            buf[0] &= 0x7F; // Clear MSB (mark as deleted)
            self.write_file_data(
                parent_cluster,
                parent_no_fat,
                parent_size,
                curr_offset,
                &buf,
            )?;
        }
        Ok(())
    }
}
