//! exFAT Cluster Allocation & FAT Management (`fat.rs`).

use alloc::vec::Vec;
use core::sync::atomic::Ordering;
use ostd::Error;

use crate::drivers::block::BlockDevice;
use crate::fs::vfs::Result;
use super::layout::BootSector;

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

pub fn get_next_cluster(block_dev: &dyn BlockDevice, boot_sector: &BootSector, cluster: u32) -> Result<u32> {
    let sector_size = 1u64 << boot_sector.bytes_per_sector_shift;
    let fat_offset_bytes = (boot_sector.fat_offset as u64) * sector_size;
    let entry_offset = fat_offset_bytes + (cluster as u64) * 4;
    let mut buf = [0u8; 4];
    read_bytes(block_dev, entry_offset, &mut buf)?;
    Ok(u32::from_le_bytes(buf))
}

pub fn set_next_cluster(block_dev: &dyn BlockDevice, boot_sector: &BootSector, cluster: u32, next: u32) -> Result<()> {
    let sector_size = 1u64 << boot_sector.bytes_per_sector_shift;
    let fat_offset_bytes = (boot_sector.fat_offset as u64) * sector_size;
    let entry_offset = fat_offset_bytes + (cluster as u64) * 4;
    let buf = next.to_le_bytes();
    write_bytes(block_dev, entry_offset, &buf)?;
    Ok(())
}

pub fn get_cluster_chain(
    block_dev: &dyn BlockDevice,
    boot_sector: &BootSector,
    first_cluster: u32,
    no_fat_chain: bool,
    size: u64,
) -> Result<Vec<u32>> {
    let mut chain = Vec::new();
    if size == 0 || first_cluster == 0 {
        return Ok(chain);
    }
    let sector_size = 1u64 << boot_sector.bytes_per_sector_shift;
    let cluster_size = sector_size * (1u64 << boot_sector.sectors_per_cluster_shift);
    let num_clusters = (size + cluster_size - 1) / cluster_size;

    if no_fat_chain {
        for i in 0..num_clusters {
            chain.push(first_cluster + i as u32);
        }
    } else {
        let mut curr = first_cluster;
        while curr >= 2 && curr < 0xFFFFFFF7 {
            chain.push(curr);
            curr = get_next_cluster(block_dev, boot_sector, curr)?;
        }
    }
    Ok(chain)
}
