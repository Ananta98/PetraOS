//! EXT2 Filesystem Mount & Superblock Management (`super.rs`).

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use ostd::Error;
use ostd::sync::SpinLock;

use crate::drivers::block::{BlockDevice, BlockDeviceInode};
use crate::fs::vfs::{Dentry, FileSystem, InodeOps, Result, SuperBlock};

use super::bitmap::{BlockBitmap, InodeBitmap};
use super::inode::Ext2Inode;
use super::layout::{EXT2_MAGIC, GroupDescriptor, Inode, Superblock};

pub fn read_blocks(
    block_dev: &dyn BlockDevice,
    block_size: u32,
    block_id: u32,
    buf: &mut [u8],
) -> Result<()> {
    let sectors_per_block = (block_size / 512) as usize;
    let start_sector = block_id as usize * sectors_per_block;
    let mut sector_buf = [0u8; 512];

    for i in 0..sectors_per_block {
        block_dev.read_blocks(start_sector + i, &mut sector_buf)?;
        let dest_offset = i * 512;
        buf[dest_offset..dest_offset + 512].copy_from_slice(&sector_buf);
    }
    Ok(())
}

pub fn write_blocks(
    block_dev: &dyn BlockDevice,
    block_size: u32,
    block_id: u32,
    buf: &[u8],
) -> Result<()> {
    let sectors_per_block = (block_size / 512) as usize;
    let start_sector = block_id as usize * sectors_per_block;
    let mut sector_buf = [0u8; 512];

    for i in 0..sectors_per_block {
        let src_offset = i * 512;
        sector_buf.copy_from_slice(&buf[src_offset..src_offset + 512]);
        block_dev.write_blocks(start_sector + i, &sector_buf)?;
    }
    Ok(())
}

pub struct Ext2FsState {
    pub block_dev: Arc<dyn BlockDevice>,
    pub superblock: Superblock,
    pub group_descriptors: SpinLock<Vec<GroupDescriptor>>,
    pub block_size: u32,
    pub groups_count: u32,
}

impl Ext2FsState {
    pub fn new(block_dev: Arc<dyn BlockDevice>, superblock: Superblock) -> Result<Self> {
        let block_size = superblock.block_size();
        let groups_count = (superblock.s_blocks_count + superblock.s_blocks_per_group - 1)
            / superblock.s_blocks_per_group;

        let gdt_start_block = if block_size == 1024 { 2 } else { 1 };
        let gdt_size_bytes = groups_count * 32;
        let blocks_needed = (gdt_size_bytes + block_size - 1) / block_size;

        let mut gdt_buf = alloc::vec![0u8; (blocks_needed * block_size) as usize];
        for i in 0..blocks_needed {
            read_blocks(
                &*block_dev,
                block_size,
                gdt_start_block + i,
                &mut gdt_buf[(i * block_size) as usize..((i + 1) * block_size) as usize],
            )?;
        }

        let mut group_descriptors = Vec::new();
        for g in 0..groups_count {
            let offset = (g * 32) as usize;
            let gd = GroupDescriptor::parse(&gdt_buf[offset..offset + 32]);
            group_descriptors.push(gd);
        }

        Ok(Self {
            block_dev,
            superblock,
            group_descriptors: SpinLock::new(group_descriptors),
            block_size,
            groups_count,
        })
    }

    pub fn write_back_gdt(&self) -> Result<()> {
        let gdt_start_block = if self.block_size == 1024 { 2 } else { 1 };
        let gdt_size_bytes = self.groups_count * 32;
        let blocks_needed = (gdt_size_bytes + self.block_size - 1) / self.block_size;

        let mut gdt_buf = alloc::vec![0u8; (blocks_needed * self.block_size) as usize];
        let descriptors = self.group_descriptors.lock();
        for g in 0..self.groups_count {
            let offset = (g * 32) as usize;
            descriptors[g as usize].serialize(&mut gdt_buf[offset..offset + 32]);
        }

        for i in 0..blocks_needed {
            write_blocks(
                &*self.block_dev,
                self.block_size,
                gdt_start_block + i,
                &gdt_buf[(i * self.block_size) as usize..((i + 1) * self.block_size) as usize],
            )?;
        }
        Ok(())
    }

    pub fn alloc_block(&self) -> Result<u32> {
        let bitmap = BlockBitmap::new(self);
        bitmap.alloc()
    }

    pub fn free_block(&self, block_id: u32) -> Result<()> {
        let bitmap = BlockBitmap::new(self);
        bitmap.free(block_id)
    }

    pub fn alloc_inode(&self, is_dir: bool) -> Result<u32> {
        let bitmap = InodeBitmap::new(self);
        bitmap.alloc(is_dir)
    }

    pub fn free_inode(&self, inode_num: u32, is_dir: bool) -> Result<()> {
        let bitmap = InodeBitmap::new(self);
        bitmap.free(inode_num, is_dir)
    }

    pub fn read_inode(&self, inode_num: u32) -> Result<Inode> {
        if inode_num == 0 || inode_num > self.superblock.s_inodes_count {
            return Err(Error::InvalidArgs);
        }
        let group = (inode_num - 1) / self.superblock.s_inodes_per_group;
        let index = (inode_num - 1) % self.superblock.s_inodes_per_group;

        let gd = self.group_descriptors.lock()[group as usize].clone();
        let inode_size = self.superblock.s_inode_size as u32;

        let byte_offset = index * inode_size;
        let block_offset = byte_offset / self.block_size;
        let offset_in_block = byte_offset % self.block_size;

        let mut block_buf = alloc::vec![0u8; self.block_size as usize];
        read_blocks(
            &*self.block_dev,
            self.block_size,
            gd.bg_inode_table + block_offset,
            &mut block_buf,
        )?;

        let slice = &block_buf[offset_in_block as usize..offset_in_block as usize + 128];
        Ok(Inode::parse(slice))
    }

    pub fn write_inode(&self, inode_num: u32, inode: &Inode) -> Result<()> {
        if inode_num == 0 || inode_num > self.superblock.s_inodes_count {
            return Err(Error::InvalidArgs);
        }
        let group = (inode_num - 1) / self.superblock.s_inodes_per_group;
        let index = (inode_num - 1) % self.superblock.s_inodes_per_group;

        let gd = self.group_descriptors.lock()[group as usize].clone();
        let inode_size = self.superblock.s_inode_size as u32;

        let byte_offset = index * inode_size;
        let block_offset = byte_offset / self.block_size;
        let offset_in_block = byte_offset % self.block_size;

        let mut block_buf = alloc::vec![0u8; self.block_size as usize];
        read_blocks(
            &*self.block_dev,
            self.block_size,
            gd.bg_inode_table + block_offset,
            &mut block_buf,
        )?;

        inode.serialize(&mut block_buf[offset_in_block as usize..offset_in_block as usize + 128]);

        write_blocks(
            &*self.block_dev,
            self.block_size,
            gd.bg_inode_table + block_offset,
            &block_buf,
        )?;
        Ok(())
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
