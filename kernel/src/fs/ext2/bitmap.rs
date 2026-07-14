use ostd::Error;

use super::ondisk::{read_blocks, write_blocks, Ext2FsState};
use crate::fs::vfs::Result;

pub(crate) struct BlockBitmap<'a> {
    fs: &'a Ext2FsState,
}

impl<'a> BlockBitmap<'a> {
    pub(crate) fn new(fs: &'a Ext2FsState) -> Self {
        Self { fs }
    }

    pub(crate) fn alloc(&self) -> Result<u32> {
        let mut descriptors = self.fs.group_descriptors.lock();
        let mut bitmap_buf = alloc::vec![0u8; self.fs.block_size as usize];

        for g in 0..self.fs.groups_count {
            if descriptors[g as usize].bg_free_blocks_count == 0 {
                continue;
            }

            let bg = &mut descriptors[g as usize];
            read_blocks(
                &*self.fs.block_dev,
                self.fs.block_size,
                bg.bg_block_bitmap,
                &mut bitmap_buf,
            )?;

            for i in 0..self.fs.block_size {
                let byte = bitmap_buf[i as usize];
                if byte != 0xFF {
                    for bit in 0..8 {
                        if (byte & (1 << bit)) == 0 {
                            let block_in_group = i * 8 + bit;
                            let absolute_block = g * self.fs.superblock.s_blocks_per_group
                                + block_in_group
                                + self.fs.superblock.s_first_data_block;

                            bitmap_buf[i as usize] |= 1 << bit;
                            write_blocks(
                                &*self.fs.block_dev,
                                self.fs.block_size,
                                bg.bg_block_bitmap,
                                &bitmap_buf,
                            )?;

                            bg.bg_free_blocks_count -= 1;
                            drop(descriptors);
                            self.fs.write_back_gdt()?;

                            let zeros = alloc::vec![0u8; self.fs.block_size as usize];
                            write_blocks(
                                &*self.fs.block_dev,
                                self.fs.block_size,
                                absolute_block,
                                &zeros,
                            )?;

                            return Ok(absolute_block);
                        }
                    }
                }
            }
        }
        Err(Error::IoError)
    }

    pub(crate) fn free(&self, block_id: u32) -> Result<()> {
        if block_id == 0 {
            return Ok(());
        }
        let block_idx_in_heap = block_id - self.fs.superblock.s_first_data_block;
        let group = block_idx_in_heap / self.fs.superblock.s_blocks_per_group;
        let block_in_group = block_idx_in_heap % self.fs.superblock.s_blocks_per_group;

        let mut descriptors = self.fs.group_descriptors.lock();
        let bg = &mut descriptors[group as usize];

        let mut bitmap_buf = alloc::vec![0u8; self.fs.block_size as usize];
        read_blocks(
            &*self.fs.block_dev,
            self.fs.block_size,
            bg.bg_block_bitmap,
            &mut bitmap_buf,
        )?;

        let byte_offset = (block_in_group / 8) as usize;
        let bit_offset = block_in_group % 8;
        bitmap_buf[byte_offset] &= !(1 << bit_offset);

        write_blocks(
            &*self.fs.block_dev,
            self.fs.block_size,
            bg.bg_block_bitmap,
            &bitmap_buf,
        )?;

        bg.bg_free_blocks_count += 1;
        drop(descriptors);
        self.fs.write_back_gdt()?;
        Ok(())
    }
}

pub(crate) struct InodeBitmap<'a> {
    fs: &'a Ext2FsState,
}

impl<'a> InodeBitmap<'a> {
    pub(crate) fn new(fs: &'a Ext2FsState) -> Self {
        Self { fs }
    }

    pub(crate) fn alloc(&self, is_dir: bool) -> Result<u32> {
        let mut descriptors = self.fs.group_descriptors.lock();
        let mut bitmap_buf = alloc::vec![0u8; self.fs.block_size as usize];

        for g in 0..self.fs.groups_count {
            if descriptors[g as usize].bg_free_inodes_count == 0 {
                continue;
            }

            let bg = &mut descriptors[g as usize];
            read_blocks(
                &*self.fs.block_dev,
                self.fs.block_size,
                bg.bg_inode_bitmap,
                &mut bitmap_buf,
            )?;

            for i in 0..self.fs.superblock.s_inodes_per_group / 8 {
                let byte = bitmap_buf[i as usize];
                if byte != 0xFF {
                    for bit in 0..8 {
                        if (byte & (1 << bit)) == 0 {
                            let inode_in_group = i * 8 + bit;
                            let absolute_inode =
                                g * self.fs.superblock.s_inodes_per_group + inode_in_group + 1;

                            bitmap_buf[i as usize] |= 1 << bit;
                            write_blocks(
                                &*self.fs.block_dev,
                                self.fs.block_size,
                                bg.bg_inode_bitmap,
                                &bitmap_buf,
                            )?;

                            bg.bg_free_inodes_count -= 1;
                            if is_dir {
                                bg.bg_used_dirs_count += 1;
                            }
                            drop(descriptors);
                            self.fs.write_back_gdt()?;

                            return Ok(absolute_inode);
                        }
                    }
                }
            }
        }
        Err(Error::IoError)
    }

    pub(crate) fn free(&self, inode_num: u32, is_dir: bool) -> Result<()> {
        if inode_num == 0 {
            return Ok(());
        }
        let group = (inode_num - 1) / self.fs.superblock.s_inodes_per_group;
        let index = (inode_num - 1) % self.fs.superblock.s_inodes_per_group;

        let mut descriptors = self.fs.group_descriptors.lock();
        let bg = &mut descriptors[group as usize];

        let mut bitmap_buf = alloc::vec![0u8; self.fs.block_size as usize];
        read_blocks(
            &*self.fs.block_dev,
            self.fs.block_size,
            bg.bg_inode_bitmap,
            &mut bitmap_buf,
        )?;

        let byte_offset = (index / 8) as usize;
        let bit_offset = index % 8;
        bitmap_buf[byte_offset] &= !(1 << bit_offset);

        write_blocks(
            &*self.fs.block_dev,
            self.fs.block_size,
            bg.bg_inode_bitmap,
            &bitmap_buf,
        )?;

        bg.bg_free_inodes_count += 1;
        if is_dir && bg.bg_used_dirs_count > 0 {
            bg.bg_used_dirs_count -= 1;
        }
        drop(descriptors);
        self.fs.write_back_gdt()?;
        Ok(())
    }
}
