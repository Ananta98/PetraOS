//! EXT2 Inode Operations (`inode.rs`).

use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;
use ostd::Error;
use ostd::sync::SpinLock;

use super::dir::{add_directory_entry, read_directory_entries, remove_directory_entry};
use super::file::{Ext2File, truncate_inode_blocks};
use super::layout::{EXT2_S_IFDIR, EXT2_S_IFREG, Inode};
use super::superblock::{Ext2FsState, write_blocks};
use crate::fs::vfs::{FileOps, FileType, InodeOps, Metadata, Result};

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

        let entries = read_directory_entries(&self.fs, self.inode_num)?;
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
        let new_inode = Inode {
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
        add_directory_entry(&self.fs, self.inode_num, name, new_ino, false)?;

        Ok(Ext2Inode::new(self.fs.clone(), new_ino, new_inode) as Arc<dyn InodeOps>)
    }

    fn mkdir(&self, name: &str, mode: u32) -> Result<Arc<dyn InodeOps>> {
        let mut parent_inode = self.inode.lock();
        let is_parent_dir = (parent_inode.i_mode & EXT2_S_IFDIR) != 0;

        if !is_parent_dir {
            return Err(Error::InvalidArgs);
        }

        drop(parent_inode);
        if self.lookup(name).is_ok() {
            return Err(Error::InvalidArgs);
        }

        let new_ino = self.fs.alloc_inode(true)?;
        let new_block = self.fs.alloc_block()?;

        let mut block_buf = alloc::vec![0u8; self.fs.block_size as usize];

        // "." entry
        block_buf[0..4].copy_from_slice(&new_ino.to_le_bytes());
        block_buf[4..6].copy_from_slice(&(12u16).to_le_bytes());
        block_buf[6] = 1;
        block_buf[7] = 2; // DIR
        block_buf[8..9].copy_from_slice(b".");

        // ".." entry
        let remaining_rec_len = self.fs.block_size - 12;
        block_buf[12..16].copy_from_slice(&self.inode_num.to_le_bytes());
        block_buf[16..18].copy_from_slice(&(remaining_rec_len as u16).to_le_bytes());
        block_buf[18] = 2;
        block_buf[19] = 2; // DIR
        block_buf[20..22].copy_from_slice(b"..");

        write_blocks(
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
            i_links_count: 2,
            i_blocks: self.fs.block_size / 512,
            i_flags: 0,
            i_block: [0; 15],
        };
        new_inode.i_block[0] = new_block;

        self.fs.write_inode(new_ino, &new_inode)?;

        add_directory_entry(&self.fs, self.inode_num, name, new_ino, true)?;

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
            uid: guard.i_uid as u32,
            gid: guard.i_gid as u32,
            inode_num: self.inode_num as u64,
            nlink: guard.i_links_count as u32,
        })
    }

    fn chmod(&self, mode: u32) -> Result<()> {
        let mut guard = self.inode.lock();
        let file_type = guard.i_mode & 0xF000;
        guard.i_mode = file_type | (mode as u16 & 0x0FFF);
        self.fs.write_inode(self.inode_num, &guard)?;
        Ok(())
    }

    fn chown(&self, uid: u32, gid: u32) -> Result<()> {
        let mut guard = self.inode.lock();
        guard.i_uid = uid as u16;
        guard.i_gid = gid as u16;
        self.fs.write_inode(self.inode_num, &guard)?;
        Ok(())
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
        let parent_inode = self.inode.lock();
        let is_parent_dir = (parent_inode.i_mode & EXT2_S_IFDIR) != 0;

        if !is_parent_dir {
            return Err(Error::InvalidArgs);
        }

        drop(parent_inode);
        let child_ino = remove_directory_entry(&self.fs, self.inode_num, name)?;

        let mut child_inode = self.fs.read_inode(child_ino)?;
        let child_is_dir = (child_inode.i_mode & EXT2_S_IFDIR) != 0;

        if child_is_dir {
            let entries = read_directory_entries(&self.fs, child_ino)?;
            if entries.len() > 2 {
                add_directory_entry(&self.fs, self.inode_num, name, child_ino, true)?;
                return Err(Error::InvalidArgs);
            }
        }

        if child_inode.i_links_count > 0 {
            child_inode.i_links_count -= 1;
        }

        if child_inode.i_links_count == 0 {
            truncate_inode_blocks(&self.fs, &mut child_inode, child_ino)?;
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
