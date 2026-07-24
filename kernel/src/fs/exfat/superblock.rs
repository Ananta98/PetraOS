//! exFAT Superblock / Volume Header & Mount Operations (`super.rs`).

use alloc::string::String;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use ostd::Error;
use ostd::sync::SpinLock;

use crate::drivers::block::{BlockDevice, BlockDeviceInode};
use crate::fs::vfs::{Dentry, FileSystem, InodeOps, Result, SuperBlock};

use super::fat::{get_cluster_chain, read_bytes, write_bytes};
use super::inode::ExFatInode;
use super::layout::{BootSector, ExFatFileInfo};

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

    pub fn get_cluster_chain(&self, first_cluster: u32, no_fat_chain: bool, size: u64) -> Result<alloc::vec::Vec<u32>> {
        get_cluster_chain(&*self.block_dev, &self.boot_sector, first_cluster, no_fat_chain, size)
    }

    pub fn alloc_cluster(&self) -> Result<u32> {
        let bitmap_first_cluster = self.bitmap_first_cluster.load(Ordering::Relaxed);
        let bitmap_size = self.bitmap_size.load(Ordering::Relaxed);

        if bitmap_first_cluster == 0 || bitmap_size == 0 {
            return Err(Error::NotEnoughResources);
        }

        let bitmap_chain = self.get_cluster_chain(bitmap_first_cluster, true, bitmap_size)?;
        let sector_size = 1u64 << self.boot_sector.bytes_per_sector_shift;
        let cluster_size = sector_size * (1u64 << self.boot_sector.sectors_per_cluster_shift);

        let mut buf = alloc::vec![0u8; cluster_size as usize];
        for (c_idx, &cluster) in bitmap_chain.iter().enumerate() {
            let sector = self.cluster_to_sector(cluster);
            read_bytes(&*self.block_dev, sector * sector_size, &mut buf)?;

            for (byte_idx, &byte) in buf.iter().enumerate() {
                if byte != 0xFF {
                    for bit in 0..8 {
                        if (byte & (1 << bit)) == 0 {
                            let cluster_index = (c_idx * cluster_size as usize + byte_idx) * 8 + bit;
                            let allocated_cluster = cluster_index as u32 + 2;

                            if allocated_cluster >= self.boot_sector.cluster_count + 2 {
                                return Err(Error::NotEnoughResources);
                            }

                            buf[byte_idx] |= 1 << bit;
                            write_bytes(&*self.block_dev, sector * sector_size, &buf)?;
                            super::fat::set_next_cluster(&*self.block_dev, &self.boot_sector, allocated_cluster, 0xFFFFFFFF)?;
                            return Ok(allocated_cluster);
                        }
                    }
                }
            }
        }
        Err(Error::NotEnoughResources)
    }

    pub fn free_cluster_chain(&self, first_cluster: u32, no_fat_chain: bool, size: u64) -> Result<()> {
        let chain = self.get_cluster_chain(first_cluster, no_fat_chain, size)?;
        let bitmap_first_cluster = self.bitmap_first_cluster.load(Ordering::Relaxed);
        let bitmap_size = self.bitmap_size.load(Ordering::Relaxed);

        let sector_size = 1u64 << self.boot_sector.bytes_per_sector_shift;
        let cluster_size = sector_size * (1u64 << self.boot_sector.sectors_per_cluster_shift);

        for cluster in chain {
            if !no_fat_chain {
                super::fat::set_next_cluster(&*self.block_dev, &self.boot_sector, cluster, 0)?;
            }
            if bitmap_first_cluster != 0 {
                let cluster_idx = (cluster - 2) as usize;
                let byte_idx = cluster_idx / 8;
                let bit_idx = cluster_idx % 8;

                let bitmap_cluster_idx = byte_idx / cluster_size as usize;
                let offset_in_cluster = byte_idx % cluster_size as usize;

                let bitmap_chain = self.get_cluster_chain(bitmap_first_cluster, true, bitmap_size)?;
                if bitmap_cluster_idx < bitmap_chain.len() {
                    let target_cluster = bitmap_chain[bitmap_cluster_idx];
                    let sector = self.cluster_to_sector(target_cluster);

                    let mut buf = alloc::vec![0u8; cluster_size as usize];
                    read_bytes(&*self.block_dev, sector * sector_size, &mut buf)?;
                    buf[offset_in_cluster] &= !(1 << bit_idx);
                    write_bytes(&*self.block_dev, sector * sector_size, &buf)?;
                }
            }
        }
        Ok(())
    }
}

pub struct ExFatFs;

impl FileSystem for ExFatFs {
    fn name(&self) -> &'static str {
        "exfat"
    }

    fn mount(&self, _flags: u32, data: &[u8]) -> Result<Arc<SuperBlock>> {
        let dev_path = core::str::from_utf8(data).map_err(|_| Error::InvalidArgs)?;
        let dev_dentry = crate::fs::vfs::path::resolve_path(dev_path)?;

        let mut target_inode = dev_dentry.inode.clone();
        if let Some(devfs_inode) = target_inode
            .as_any()
            .downcast_ref::<crate::fs::devfs::DevfsInode>()
        {
            if let Some(wrapped_device) = devfs_inode.device() {
                target_inode = wrapped_device;
            }
        }

        let block_inode = target_inode
            .as_any()
            .downcast_ref::<BlockDeviceInode>()
            .ok_or(Error::InvalidArgs)?;
        let block_dev = block_inode.device.clone();

        let mut boot_bytes = [0u8; 512];
        read_bytes(&*block_dev, 0, &mut boot_bytes)?;

        let mut jump_boot = [0u8; 3];
        jump_boot.copy_from_slice(&boot_bytes[0..3]);
        let mut fs_name = [0u8; 8];
        fs_name.copy_from_slice(&boot_bytes[3..11]);
        let mut must_be_zero = [0u8; 53];
        must_be_zero.copy_from_slice(&boot_bytes[11..64]);
        let partition_offset = u64::from_le_bytes([
            boot_bytes[64],
            boot_bytes[65],
            boot_bytes[66],
            boot_bytes[67],
            boot_bytes[68],
            boot_bytes[69],
            boot_bytes[70],
            boot_bytes[71],
        ]);
        let volume_length = u64::from_le_bytes([
            boot_bytes[72],
            boot_bytes[73],
            boot_bytes[74],
            boot_bytes[75],
            boot_bytes[76],
            boot_bytes[77],
            boot_bytes[78],
            boot_bytes[79],
        ]);
        let fat_offset = u32::from_le_bytes([
            boot_bytes[80],
            boot_bytes[81],
            boot_bytes[82],
            boot_bytes[83],
        ]);
        let fat_length = u32::from_le_bytes([
            boot_bytes[84],
            boot_bytes[85],
            boot_bytes[86],
            boot_bytes[87],
        ]);
        let cluster_heap_offset = u32::from_le_bytes([
            boot_bytes[88],
            boot_bytes[89],
            boot_bytes[90],
            boot_bytes[91],
        ]);
        let cluster_count = u32::from_le_bytes([
            boot_bytes[92],
            boot_bytes[93],
            boot_bytes[94],
            boot_bytes[95],
        ]);
        let first_cluster_of_root = u32::from_le_bytes([
            boot_bytes[96],
            boot_bytes[97],
            boot_bytes[98],
            boot_bytes[99],
        ]);
        let mut volume_guid = [0u8; 16];
        volume_guid.copy_from_slice(&boot_bytes[100..116]);
        let fs_revision = u16::from_le_bytes([boot_bytes[116], boot_bytes[117]]);
        let flags = u16::from_le_bytes([boot_bytes[118], boot_bytes[119]]);
        let bytes_per_sector_shift = boot_bytes[120];
        let sectors_per_cluster_shift = boot_bytes[121];
        let number_of_fats = boot_bytes[122];
        let drive_select = boot_bytes[123];
        let percent_in_use = boot_bytes[124];
        let mut reserved = [0u8; 7];
        reserved.copy_from_slice(&boot_bytes[125..132]);
        let mut boot_code = [0u8; 378];
        boot_code.copy_from_slice(&boot_bytes[132..510]);
        let signature = u16::from_le_bytes([boot_bytes[510], boot_bytes[511]]);

        let boot_sector = BootSector {
            jump_boot,
            fs_name,
            must_be_zero,
            partition_offset,
            volume_length,
            fat_offset,
            fat_length,
            cluster_heap_offset,
            cluster_count,
            first_cluster_of_root,
            volume_guid,
            fs_revision,
            flags,
            bytes_per_sector_shift,
            sectors_per_cluster_shift,
            number_of_fats,
            drive_select,
            percent_in_use,
            reserved,
            boot_code,
            signature,
        };

        if &boot_sector.fs_name != b"EXFAT   " {
            return Err(Error::IoError);
        }

        let sector_size = 1u64 << boot_sector.bytes_per_sector_shift;
        let cluster_size = sector_size * (1u64 << boot_sector.sectors_per_cluster_shift);

        let fs_state = Arc::new(ExFatFsState {
            block_dev,
            boot_sector,
            bitmap_first_cluster: AtomicU32::new(0),
            bitmap_size: AtomicU64::new(0),
            root_info: SpinLock::new(ExFatFileInfo {
                name: String::from("/"),
                file_attributes: 0x10,
                first_cluster: boot_sector.first_cluster_of_root,
                size: cluster_size,
                is_dir: true,
                no_fat_chain: true,
                entry_cluster: 0,
                entry_offset_in_dir: 0,
                entry_count: 0,
            }),
        });

        let entries = super::dir::read_directory_entries(
            &fs_state,
            boot_sector.first_cluster_of_root,
            true,
            cluster_size,
        )?;

        for file in &entries {
            if file.file_attributes == 0 {
                let mut entry_buf = [0u8; 32];
                if super::file::read_file_data(
                    &fs_state,
                    boot_sector.first_cluster_of_root,
                    true,
                    cluster_size,
                    file.entry_offset_in_dir as u64,
                    &mut entry_buf,
                )
                .is_ok()
                {
                    if entry_buf[0] == 0x81 {
                        let first_cluster = u32::from_le_bytes([
                            entry_buf[20],
                            entry_buf[21],
                            entry_buf[22],
                            entry_buf[23],
                        ]);
                        let size = u64::from_le_bytes([
                            entry_buf[24],
                            entry_buf[25],
                            entry_buf[26],
                            entry_buf[27],
                            entry_buf[28],
                            entry_buf[29],
                            entry_buf[30],
                            entry_buf[31],
                        ]);
                        fs_state
                            .bitmap_first_cluster
                            .store(first_cluster, Ordering::Relaxed);
                        fs_state.bitmap_size.store(size, Ordering::Relaxed);
                    }
                }
            }
        }

        let root_info = fs_state.root_info.lock().clone();
        let root_inode = ExFatInode::new(fs_state, root_info);

        let sb = Arc::new(SuperBlock {
            fs_type: String::from(self.name()),
            root_dentry: SpinLock::new(None),
        });
        let root_dentry = Dentry::new("/", root_inode as Arc<dyn InodeOps>, None);
        *sb.root_dentry.lock() = Some(root_dentry);

        Ok(sb)
    }
}
