//! exFAT Inode Operations (`inode.rs`).

use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;
use ostd::Error;
use ostd::sync::SpinLock;

use super::dir::{
    delete_dir_entry_set, find_free_dir_slots, read_directory_entries, write_dir_entry_set,
};
use super::fat::write_bytes;
use super::file::ExFatFile;
use super::layout::ExFatFileInfo;
use super::superblock::ExFatFsState;
use crate::fs::vfs::{FileOps, FileType, InodeOps, Metadata, Result};

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
            read_directory_entries(&self.fs, info.first_cluster, info.no_fat_chain, info.size)?;
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

        let files = read_directory_entries(
            &self.fs,
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

        let start_offset = find_free_dir_slots(
            &self.fs,
            dir_cluster,
            &mut dir_no_fat,
            &mut dir_size,
            parent_parent,
            false,
            parent_entry_offset,
            slots_needed,
        )?;

        let mut parent_info = self.file_info.lock();
        parent_info.no_fat_chain = dir_no_fat;
        parent_info.size = dir_size;

        write_dir_entry_set(
            &self.fs,
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

        let files = read_directory_entries(
            &self.fs,
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

        let start_offset = find_free_dir_slots(
            &self.fs,
            dir_cluster,
            &mut dir_no_fat,
            &mut dir_size,
            parent_parent,
            false,
            parent_entry_offset,
            slots_needed,
        )?;

        let mut parent_info = self.file_info.lock();
        parent_info.no_fat_chain = dir_no_fat;
        parent_info.size = dir_size;

        write_dir_entry_set(
            &self.fs,
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
            uid: 0,
            gid: 0,
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

        let files = read_directory_entries(
            &self.fs,
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
                delete_dir_entry_set(
                    &self.fs,
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
