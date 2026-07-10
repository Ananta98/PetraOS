/// EXT2 VFS Integration
///
/// Implements VFS traits (FileSystem, InodeOps, and FileOps) to expose EXT2 operations
/// to the PetraOS kernel.
use crate::drivers::block::{BlockDevice, BlockDeviceInode};
use crate::fs::vfs::{
    Dentry, DirEntry, FileOps, FileSystem, FileType, InodeOps, Metadata, Result, SeekFrom,
    SuperBlock,
};
use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use ostd::Error;
use ostd::sync::SpinLock;

use super::ondisk::{Ext2FsState, read_blocks};
use super::structs::{EXT2_MAGIC, EXT2_S_IFDIR, EXT2_S_IFREG, Inode, Superblock};

pub struct Ext2Inode {
    pub fs: Arc<Ext2FsState>,
    pub inode_num: u32,
    pub inode: SpinLock<Inode>,
}

impl Ext2Inode {
    pub fn new(fs: Arc<Ext2FsState>, inode_num: u32, inode: Inode) -> Arc<Self> {
        Arc::new(Self {
            fs,
            inode_num,
            inode: SpinLock::new(inode),
        })
    }
}

impl InodeOps for Ext2Inode {
    fn lookup(&self, name: &str) -> Result<Arc<dyn InodeOps>> {
        let is_dir = {
            let guard = self.inode.lock();
            (guard.i_mode & EXT2_S_IFDIR) != 0
        };

        if !is_dir {
            return Err(Error::InvalidArgs);
        }

        let entries = self.fs.read_directory_entries(self.inode_num)?;
        for entry in entries {
            if entry.name == name {
                let child_inode = self.fs.read_inode(entry.inode_num)?;
                return Ok(
                    Ext2Inode::new(self.fs.clone(), entry.inode_num, child_inode)
                        as Arc<dyn InodeOps>,
                );
            }
        }
        Err(Error::InvalidArgs)
    }

    fn create(&self, name: &str, mode: u32) -> Result<Arc<dyn InodeOps>> {
        let is_dir = {
            let guard = self.inode.lock();
            (guard.i_mode & EXT2_S_IFDIR) != 0
        };

        if !is_dir {
            return Err(Error::InvalidArgs);
        }

        // Check if file already exists
        if self.lookup(name).is_ok() {
            return Err(Error::InvalidArgs);
        }

        // Allocate a new inode
        let new_ino = self.fs.alloc_inode(false)?;
        let mut new_inode = Inode {
            i_mode: EXT2_S_IFREG | (mode as u16 & 0x1FF),
            i_uid: 0,
            i_size: 0,
            i_atime: 0,
            i_ctime: 0,
            i_mtime: 0,
            i_dtime: 0,
            i_gid: 0,
            i_links_count: 1,
            i_blocks: 0,
            i_flags: 0,
            i_block: [0; 15],
        };

        self.fs.write_inode(new_ino, &new_inode)?;

        // Add directory entry
        self.fs
            .add_directory_entry(self.inode_num, name, new_ino, false)?;

        Ok(Ext2Inode::new(self.fs.clone(), new_ino, new_inode) as Arc<dyn InodeOps>)
    }

    fn mkdir(&self, name: &str, mode: u32) -> Result<Arc<dyn InodeOps>> {
        let mut parent_inode = self.inode.lock();
        let is_parent_dir = (parent_inode.i_mode & EXT2_S_IFDIR) != 0;

        if !is_parent_dir {
            return Err(Error::InvalidArgs);
        }

        // Check if file already exists
        // Release parent lock before lookup to avoid deadlock
        drop(parent_inode);
        if self.lookup(name).is_ok() {
            return Err(Error::InvalidArgs);
        }

        // Allocate new inode
        let new_ino = self.fs.alloc_inode(true)?;
        let new_block = self.fs.alloc_block()?;

        // Write dot and dot-dot entries to the newly allocated block
        let mut block_buf = alloc::vec![0u8; self.fs.block_size as usize];

        // "." entry pointing to self
        block_buf[0..4].copy_from_slice(&new_ino.to_le_bytes());
        block_buf[4..6].copy_from_slice(&(12u16).to_le_bytes()); // rec_len
        block_buf[6] = 1; // name_len
        block_buf[7] = 2; // file_type (DIR)
        block_buf[8..9].copy_from_slice(b".");

        // ".." entry pointing to parent
        let remaining_rec_len = self.fs.block_size - 12;
        block_buf[12..16].copy_from_slice(&self.inode_num.to_le_bytes());
        block_buf[16..18].copy_from_slice(&(remaining_rec_len as u16).to_le_bytes());
        block_buf[18] = 2; // name_len
        block_buf[19] = 2; // file_type (DIR)
        block_buf[20..22].copy_from_slice(b"..");

        super::ondisk::write_blocks(
            &*self.fs.block_dev,
            self.fs.block_size,
            new_block,
            &block_buf,
        )?;

        let mut new_inode = Inode {
            i_mode: EXT2_S_IFDIR | (mode as u16 & 0x1FF),
            i_uid: 0,
            i_size: self.fs.block_size,
            i_atime: 0,
            i_ctime: 0,
            i_mtime: 0,
            i_dtime: 0,
            i_gid: 0,
            i_links_count: 2, // "." and parent link
            i_blocks: self.fs.block_size / 512,
            i_flags: 0,
            i_block: [0; 15],
        };
        new_inode.i_block[0] = new_block;

        self.fs.write_inode(new_ino, &new_inode)?;

        // Add entry in parent directory
        self.fs
            .add_directory_entry(self.inode_num, name, new_ino, true)?;

        // Increment link count of parent directory
        let mut parent_inode = self.inode.lock();
        parent_inode.i_links_count += 1;
        self.fs.write_inode(self.inode_num, &parent_inode)?;

        Ok(Ext2Inode::new(self.fs.clone(), new_ino, new_inode) as Arc<dyn InodeOps>)
    }

    fn symlink(&self, _name: &str, _target: &str) -> Result<Arc<dyn InodeOps>> {
        Err(Error::InvalidArgs)
    }

    fn metadata(&self) -> Result<Metadata> {
        let guard = self.inode.lock();
        let file_type = if (guard.i_mode & EXT2_S_IFDIR) != 0 {
            FileType::Directory
        } else {
            FileType::Regular
        };

        Ok(Metadata {
            size: guard.i_size as usize,
            file_type,
            mode: guard.i_mode as u32,
            inode_num: self.inode_num as u64,
            nlink: guard.i_links_count as u32,
        })
    }

    fn read_link(&self) -> Result<String> {
        Err(Error::InvalidArgs)
    }

    fn open(&self, _flags: u32) -> Result<Box<dyn FileOps>> {
        let guard = self.inode.lock();
        Ok(Box::new(Ext2File {
            inode: Arc::new(Self {
                fs: self.fs.clone(),
                inode_num: self.inode_num,
                inode: SpinLock::new(guard.clone()),
            }),
        }))
    }

    fn as_any(&self) -> &dyn core::any::Any {
        self
    }

    fn unlink(&self, name: &str) -> Result<()> {
        let mut parent_inode = self.inode.lock();
        let is_parent_dir = (parent_inode.i_mode & EXT2_S_IFDIR) != 0;

        if !is_parent_dir {
            return Err(Error::InvalidArgs);
        }

        // Release lock before lookup and remove to avoid deadlock
        drop(parent_inode);
        let child_ino = self.fs.remove_directory_entry(self.inode_num, name)?;

        let mut child_inode = self.fs.read_inode(child_ino)?;
        let child_is_dir = (child_inode.i_mode & EXT2_S_IFDIR) != 0;

        if child_is_dir {
            // Check if directory is empty (must have only . and ..)
            let entries = self.fs.read_directory_entries(child_ino)?;
            if entries.len() > 2 {
                // Not empty, rollback directory removal
                self.fs
                    .add_directory_entry(self.inode_num, name, child_ino, true)?;
                return Err(Error::InvalidArgs);
            }
        }

        if child_inode.i_links_count > 0 {
            child_inode.i_links_count -= 1;
        }

        if child_inode.i_links_count == 0 {
            // Free all data blocks
            self.fs.truncate_inode_blocks(&mut child_inode, child_ino)?;
            // Free inode in bitmap
            self.fs.free_inode(child_ino, child_is_dir)?;
        } else {
            self.fs.write_inode(child_ino, &child_inode)?;
        }

        if child_is_dir {
            let mut parent_inode = self.inode.lock();
            if parent_inode.i_links_count > 0 {
                parent_inode.i_links_count -= 1;
            }
            self.fs.write_inode(self.inode_num, &parent_inode)?;
        }

        Ok(())
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

pub struct Ext2File {
    pub inode: Arc<Ext2Inode>,
}

impl FileOps for Ext2File {
    fn read(&mut self, buf: &mut [u8], offset: &mut usize) -> Result<usize> {
        let mut guard = self.inode.inode.lock();
        let bytes_read =
            self.inode
                .fs
                .read_file_data(&mut guard, self.inode.inode_num, *offset as u64, buf)?;
        *offset += bytes_read;
        Ok(bytes_read)
    }

    fn write(&mut self, buf: &[u8], offset: &mut usize) -> Result<usize> {
        let mut guard = self.inode.inode.lock();
        let bytes_written =
            self.inode
                .fs
                .write_file_data(&mut guard, self.inode.inode_num, *offset as u64, buf)?;
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
        let entries = self.inode.fs.read_directory_entries(self.inode.inode_num)?;
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

pub struct Ext2Fs;

impl FileSystem for Ext2Fs {
    fn name(&self) -> &'static str {
        "ext2"
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

        // Read Superblock (located at 1024 bytes offset, regardless of block size)
        let mut sb_buf = [0u8; 1024];
        let mut sector_buf = [0u8; 512];
        block_dev.read_blocks(2, &mut sector_buf)?;
        sb_buf[0..512].copy_from_slice(&sector_buf);
        block_dev.read_blocks(3, &mut sector_buf)?;
        sb_buf[512..1024].copy_from_slice(&sector_buf);

        let superblock = Superblock::parse(&sb_buf);
        if superblock.s_magic != EXT2_MAGIC {
            return Err(Error::IoError);
        }

        let fs_state = Arc::new(Ext2FsState::new(block_dev, superblock)?);

        // Read Root Inode (always inode 2 in EXT2)
        let root_inode_data = fs_state.read_inode(2)?;
        let root_inode = Ext2Inode::new(fs_state, 2, root_inode_data);

        let sb = Arc::new(SuperBlock {
            fs_type: String::from(self.name()),
            root_dentry: SpinLock::new(None),
        });
        let root_dentry = Dentry::new("/", root_inode as Arc<dyn InodeOps>, None);
        *sb.root_dentry.lock() = Some(root_dentry);

        Ok(sb)
    }
}
