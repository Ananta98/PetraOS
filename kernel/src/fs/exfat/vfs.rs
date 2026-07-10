use crate::drivers::block::{BlockDevice, BlockDeviceInode};
use crate::fs::vfs::{
    Dentry, DirEntry, FileOps, FileSystem, FileType, InodeOps, Metadata, Result, SeekFrom,
    SuperBlock,
};
use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use ostd::Error;
use ostd::sync::SpinLock;

use super::ondisk::{ExFatFsState, read_bytes, write_bytes};
use super::structs::{BootSector, ExFatFileInfo};

pub struct ExFatInode {
    pub fs: Arc<ExFatFsState>,
    pub file_info: SpinLock<ExFatFileInfo>,
}

impl ExFatInode {
    pub fn new(fs: Arc<ExFatFsState>, file_info: ExFatFileInfo) -> Arc<Self> {
        Arc::new(Self {
            fs,
            file_info: SpinLock::new(file_info),
        })
    }
}

impl InodeOps for ExFatInode {
    fn lookup(&self, name: &str) -> Result<Arc<dyn InodeOps>> {
        let info = self.file_info.lock();
        if !info.is_dir {
            return Err(Error::InvalidArgs);
        }
        let files =
            self.fs
                .read_directory_entries(info.first_cluster, info.no_fat_chain, info.size)?;
        for file in files {
            if file.name == name {
                return Ok(ExFatInode::new(self.fs.clone(), file) as Arc<dyn InodeOps>);
            }
        }
        Err(Error::InvalidArgs)
    }

    fn create(&self, name: &str, _mode: u32) -> Result<Arc<dyn InodeOps>> {
        let parent_info = self.file_info.lock();
        if !parent_info.is_dir {
            return Err(Error::InvalidArgs);
        }

        // Check if file already exists
        let files = self.fs.read_directory_entries(
            parent_info.first_cluster,
            parent_info.no_fat_chain,
            parent_info.size,
        )?;
        for file in &files {
            if file.name == name {
                return Err(Error::InvalidArgs);
            }
        }

        let name_entries = (name.encode_utf16().count() + 14) / 15;
        let slots_needed = 2 + name_entries;

        let parent_parent = parent_info.entry_cluster;

        let dir_cluster = parent_info.first_cluster;
        let mut dir_no_fat = parent_info.no_fat_chain;
        let mut dir_size = parent_info.size;
        let parent_entry_offset = parent_info.entry_offset_in_dir;
        drop(parent_info);

        let start_offset = self.fs.find_free_dir_slots(
            dir_cluster,
            &mut dir_no_fat,
            &mut dir_size,
            parent_parent,
            false,
            0,
            parent_entry_offset,
            slots_needed,
        )?;

        let mut parent_info = self.file_info.lock();
        parent_info.no_fat_chain = dir_no_fat;
        parent_info.size = dir_size;

        self.fs.write_dir_entry_set(
            parent_info.first_cluster,
            parent_info.no_fat_chain,
            parent_info.size,
            start_offset,
            name,
            0x20, // Archive attribute
            0,
            0,
        )?;

        let child_info = ExFatFileInfo {
            name: String::from(name),
            file_attributes: 0x20,
            first_cluster: 0,
            size: 0,
            is_dir: false,
            no_fat_chain: true,
            entry_cluster: parent_info.first_cluster,
            entry_offset_in_dir: start_offset,
            entry_count: slots_needed,
        };

        Ok(ExFatInode::new(self.fs.clone(), child_info) as Arc<dyn InodeOps>)
    }

    fn mkdir(&self, name: &str, _mode: u32) -> Result<Arc<dyn InodeOps>> {
        let parent_info = self.file_info.lock();
        if !parent_info.is_dir {
            return Err(Error::InvalidArgs);
        }

        let files = self.fs.read_directory_entries(
            parent_info.first_cluster,
            parent_info.no_fat_chain,
            parent_info.size,
        )?;
        for file in &files {
            if file.name == name {
                return Err(Error::InvalidArgs);
            }
        }

        let name_entries = (name.encode_utf16().count() + 14) / 15;
        let slots_needed = 2 + name_entries;

        let new_cluster = self.fs.alloc_cluster()?;
        let sector_size = 1u64 << self.fs.boot_sector.bytes_per_sector_shift;
        let cluster_size = sector_size * (1u64 << self.fs.boot_sector.sectors_per_cluster_shift);

        let zeros = alloc::vec![0u8; cluster_size as usize];
        write_bytes(
            &*self.fs.block_dev,
            self.fs.cluster_to_sector(new_cluster) * sector_size,
            &zeros,
        )?;

        let parent_parent = parent_info.entry_cluster;

        let dir_cluster = parent_info.first_cluster;
        let mut dir_no_fat = parent_info.no_fat_chain;
        let mut dir_size = parent_info.size;
        let parent_entry_offset = parent_info.entry_offset_in_dir;
        drop(parent_info);

        let start_offset = self.fs.find_free_dir_slots(
            dir_cluster,
            &mut dir_no_fat,
            &mut dir_size,
            parent_parent,
            false,
            0,
            parent_entry_offset,
            slots_needed,
        )?;

        let mut parent_info = self.file_info.lock();
        parent_info.no_fat_chain = dir_no_fat;
        parent_info.size = dir_size;

        self.fs.write_dir_entry_set(
            parent_info.first_cluster,
            parent_info.no_fat_chain,
            parent_info.size,
            start_offset,
            name,
            0x10, // Directory attribute
            new_cluster,
            cluster_size,
        )?;

        let child_info = ExFatFileInfo {
            name: String::from(name),
            file_attributes: 0x10,
            first_cluster: new_cluster,
            size: cluster_size,
            is_dir: true,
            no_fat_chain: true,
            entry_cluster: parent_info.first_cluster,
            entry_offset_in_dir: start_offset,
            entry_count: slots_needed,
        };

        Ok(ExFatInode::new(self.fs.clone(), child_info) as Arc<dyn InodeOps>)
    }

    fn symlink(&self, _name: &str, _target: &str) -> Result<Arc<dyn InodeOps>> {
        Err(Error::InvalidArgs)
    }

    fn metadata(&self) -> Result<Metadata> {
        let info = self.file_info.lock();
        let file_type = if info.is_dir {
            FileType::Directory
        } else {
            FileType::Regular
        };
        let inode_num = if info.first_cluster != 0 {
            info.first_cluster as u64
        } else {
            ((info.entry_cluster as u64) << 32) | (info.entry_offset_in_dir as u64)
        };

        Ok(Metadata {
            size: info.size as usize,
            file_type,
            mode: if info.is_dir { 0o755 } else { 0o644 },
            inode_num,
            nlink: 1,
        })
    }

    fn read_link(&self) -> Result<String> {
        Err(Error::InvalidArgs)
    }

    fn open(&self, _flags: u32) -> Result<Box<dyn FileOps>> {
        Ok(Box::new(ExFatFile {
            inode: Arc::new(Self {
                fs: self.fs.clone(),
                file_info: SpinLock::new(self.file_info.lock().clone()),
            }),
        }))
    }

    fn as_any(&self) -> &dyn core::any::Any {
        self
    }

    fn unlink(&self, name: &str) -> Result<()> {
        let parent_info = self.file_info.lock();
        if !parent_info.is_dir {
            return Err(Error::InvalidArgs);
        }

        let files = self.fs.read_directory_entries(
            parent_info.first_cluster,
            parent_info.no_fat_chain,
            parent_info.size,
        )?;
        for file in files {
            if file.name == name {
                if file.first_cluster != 0 {
                    self.fs
                        .free_cluster_chain(file.first_cluster, file.no_fat_chain, file.size)?;
                }
                self.fs.delete_dir_entry_set(
                    parent_info.first_cluster,
                    parent_info.no_fat_chain,
                    parent_info.size,
                    file.entry_offset_in_dir,
                    file.entry_count,
                )?;
                return Ok(());
            }
        }
        Err(Error::InvalidArgs)
    }

    fn rename(
        &self,
        _old_name: &str,
        _new_parent: &Arc<dyn InodeOps>,
        _new_name: &str,
    ) -> Result<()> {
        Err(Error::InvalidArgs)
    }
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
        let bytes_read = self.inode.fs.read_file_data(
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

            self.inode.fs.extend_file(
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

        let bytes_written = self.inode.fs.write_file_data(
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

        let files = self.inode.fs.read_directory_entries(
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

        let entries = fs_state.read_directory_entries(
            boot_sector.first_cluster_of_root,
            true,
            cluster_size,
        )?;

        for file in &entries {
            if file.file_attributes == 0 {
                let mut entry_buf = [0u8; 32];
                if fs_state
                    .read_file_data(
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
