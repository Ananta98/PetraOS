/// EXT2 On-Disk Structure Helpers
///
/// Implements block layout translation, inode reading/writing, allocator (blocks & inodes),
/// directory entry parsing, and file data read/write routines.
use crate::drivers::block::BlockDevice;
use crate::fs::vfs::Result;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use ostd::Error;
use ostd::sync::SpinLock;

use super::structs::{
    EXT2_FT_DIR, EXT2_FT_REG_FILE, EXT2_S_IFDIR, EXT2_S_IFREG, GroupDescriptor, Inode, Superblock,
};

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

#[derive(Debug, Clone)]
pub struct Ext2FileInfo {
    pub name: String,
    pub inode_num: u32,
    pub mode: u16,
    pub size: u32,
    pub is_dir: bool,
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

        // Group Descriptor Table starts at block 2 if block size is 1KB, or block 1 if block size > 1KB
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

        let inode_slice =
            &block_buf[offset_in_block as usize..offset_in_block as usize + inode_size as usize];
        Ok(Inode::parse(inode_slice))
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

        let inode_slice = &mut block_buf
            [offset_in_block as usize..offset_in_block as usize + inode_size as usize];
        inode.serialize(inode_slice);

        write_blocks(
            &*self.block_dev,
            self.block_size,
            gd.bg_inode_table + block_offset,
            &block_buf,
        )?;
        Ok(())
    }

    // Allocate a block and return its physical block number
    pub fn alloc_block(&self) -> Result<u32> {
        let mut descriptors = self.group_descriptors.lock();
        let mut bitmap_buf = alloc::vec![0u8; self.block_size as usize];

        for g in 0..self.groups_count {
            if descriptors[g as usize].bg_free_blocks_count == 0 {
                continue;
            }

            let bg = &mut descriptors[g as usize];
            read_blocks(
                &*self.block_dev,
                self.block_size,
                bg.bg_block_bitmap,
                &mut bitmap_buf,
            )?;

            for i in 0..self.block_size {
                let byte = bitmap_buf[i as usize];
                if byte != 0xFF {
                    for bit in 0..8 {
                        if (byte & (1 << bit)) == 0 {
                            let block_in_group = i * 8 + bit;
                            let absolute_block = g * self.superblock.s_blocks_per_group
                                + block_in_group
                                + self.superblock.s_first_data_block;

                            // Mark as used
                            bitmap_buf[i as usize] |= 1 << bit;
                            write_blocks(
                                &*self.block_dev,
                                self.block_size,
                                bg.bg_block_bitmap,
                                &bitmap_buf,
                            )?;

                            bg.bg_free_blocks_count -= 1;
                            drop(descriptors);
                            self.write_back_gdt()?;

                            // Zero out the newly allocated block
                            let zeros = alloc::vec![0u8; self.block_size as usize];
                            write_blocks(
                                &*self.block_dev,
                                self.block_size,
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

    // Free a block
    pub fn free_block(&self, block_id: u32) -> Result<()> {
        if block_id == 0 {
            return Ok(());
        }
        let block_idx_in_heap = block_id - self.superblock.s_first_data_block;
        let group = block_idx_in_heap / self.superblock.s_blocks_per_group;
        let block_in_group = block_idx_in_heap % self.superblock.s_blocks_per_group;

        let mut descriptors = self.group_descriptors.lock();
        let bg = &mut descriptors[group as usize];

        let mut bitmap_buf = alloc::vec![0u8; self.block_size as usize];
        read_blocks(
            &*self.block_dev,
            self.block_size,
            bg.bg_block_bitmap,
            &mut bitmap_buf,
        )?;

        let byte_offset = (block_in_group / 8) as usize;
        let bit_offset = block_in_group % 8;
        bitmap_buf[byte_offset] &= !(1 << bit_offset);

        write_blocks(
            &*self.block_dev,
            self.block_size,
            bg.bg_block_bitmap,
            &bitmap_buf,
        )?;

        bg.bg_free_blocks_count += 1;
        drop(descriptors);
        self.write_back_gdt()?;
        Ok(())
    }

    // Allocate an inode and return its index
    pub fn alloc_inode(&self, is_dir: bool) -> Result<u32> {
        let mut descriptors = self.group_descriptors.lock();
        let mut bitmap_buf = alloc::vec![0u8; self.block_size as usize];

        for g in 0..self.groups_count {
            if descriptors[g as usize].bg_free_inodes_count == 0 {
                continue;
            }

            let bg = &mut descriptors[g as usize];
            read_blocks(
                &*self.block_dev,
                self.block_size,
                bg.bg_inode_bitmap,
                &mut bitmap_buf,
            )?;

            for i in 0..self.superblock.s_inodes_per_group / 8 {
                let byte = bitmap_buf[i as usize];
                if byte != 0xFF {
                    for bit in 0..8 {
                        if (byte & (1 << bit)) == 0 {
                            let inode_in_group = i * 8 + bit;
                            let absolute_inode =
                                g * self.superblock.s_inodes_per_group + inode_in_group + 1;

                            // Mark as used
                            bitmap_buf[i as usize] |= 1 << bit;
                            write_blocks(
                                &*self.block_dev,
                                self.block_size,
                                bg.bg_inode_bitmap,
                                &bitmap_buf,
                            )?;

                            bg.bg_free_inodes_count -= 1;
                            if is_dir {
                                bg.bg_used_dirs_count += 1;
                            }
                            drop(descriptors);
                            self.write_back_gdt()?;

                            return Ok(absolute_inode);
                        }
                    }
                }
            }
        }
        Err(Error::IoError)
    }

    // Free an inode
    pub fn free_inode(&self, inode_num: u32, is_dir: bool) -> Result<()> {
        if inode_num == 0 {
            return Ok(());
        }
        let group = (inode_num - 1) / self.superblock.s_inodes_per_group;
        let index = (inode_num - 1) % self.superblock.s_inodes_per_group;

        let mut descriptors = self.group_descriptors.lock();
        let bg = &mut descriptors[group as usize];

        let mut bitmap_buf = alloc::vec![0u8; self.block_size as usize];
        read_blocks(
            &*self.block_dev,
            self.block_size,
            bg.bg_inode_bitmap,
            &mut bitmap_buf,
        )?;

        let byte_offset = (index / 8) as usize;
        let bit_offset = index % 8;
        bitmap_buf[byte_offset] &= !(1 << bit_offset);

        write_blocks(
            &*self.block_dev,
            self.block_size,
            bg.bg_inode_bitmap,
            &bitmap_buf,
        )?;

        bg.bg_free_inodes_count += 1;
        if is_dir && bg.bg_used_dirs_count > 0 {
            bg.bg_used_dirs_count -= 1;
        }
        drop(descriptors);
        self.write_back_gdt()?;
        Ok(())
    }

    // Block translation: logical block index in inode to physical block ID on disk
    pub fn translate_block(
        &self,
        inode: &mut Inode,
        inode_num: u32,
        logical_block: u32,
        alloc: bool,
    ) -> Result<u32> {
        let pointers_per_block = self.block_size / 4;

        if logical_block < 12 {
            let mut p_block = inode.i_block[logical_block as usize];
            if p_block == 0 && alloc {
                p_block = self.alloc_block()?;
                inode.i_block[logical_block as usize] = p_block;
                inode.i_blocks += self.block_size / 512;
                self.write_inode(inode_num, inode)?;
            }
            return Ok(p_block);
        }

        let indirect_start = 12;
        if logical_block < indirect_start + pointers_per_block {
            let offset = logical_block - indirect_start;
            let mut single_indirect = inode.i_block[12];
            if single_indirect == 0 {
                if !alloc {
                    return Ok(0);
                }
                single_indirect = self.alloc_block()?;
                inode.i_block[12] = single_indirect;
                inode.i_blocks += self.block_size / 512;
                self.write_inode(inode_num, inode)?;
            }

            let mut indirect_buf = alloc::vec![0u8; self.block_size as usize];
            read_blocks(
                &*self.block_dev,
                self.block_size,
                single_indirect,
                &mut indirect_buf,
            )?;

            let byte_offset = (offset * 4) as usize;
            let mut p_block = u32::from_le_bytes([
                indirect_buf[byte_offset],
                indirect_buf[byte_offset + 1],
                indirect_buf[byte_offset + 2],
                indirect_buf[byte_offset + 3],
            ]);

            if p_block == 0 && alloc {
                p_block = self.alloc_block()?;
                indirect_buf[byte_offset..byte_offset + 4].copy_from_slice(&p_block.to_le_bytes());
                write_blocks(
                    &*self.block_dev,
                    self.block_size,
                    single_indirect,
                    &indirect_buf,
                )?;
                inode.i_blocks += self.block_size / 512;
                self.write_inode(inode_num, inode)?;
            }
            return Ok(p_block);
        }

        let doubly_indirect_start = indirect_start + pointers_per_block;
        if logical_block < doubly_indirect_start + pointers_per_block * pointers_per_block {
            let offset = logical_block - doubly_indirect_start;
            let idx1 = offset / pointers_per_block;
            let idx2 = offset % pointers_per_block;

            let mut double_indirect = inode.i_block[13];
            if double_indirect == 0 {
                if !alloc {
                    return Ok(0);
                }
                double_indirect = self.alloc_block()?;
                inode.i_block[13] = double_indirect;
                inode.i_blocks += self.block_size / 512;
                self.write_inode(inode_num, inode)?;
            }

            let mut double_buf = alloc::vec![0u8; self.block_size as usize];
            read_blocks(
                &*self.block_dev,
                self.block_size,
                double_indirect,
                &mut double_buf,
            )?;

            let offset1 = (idx1 * 4) as usize;
            let mut single_indirect = u32::from_le_bytes([
                double_buf[offset1],
                double_buf[offset1 + 1],
                double_buf[offset1 + 2],
                double_buf[offset1 + 3],
            ]);

            let mut double_updated = false;
            if single_indirect == 0 {
                if !alloc {
                    return Ok(0);
                }
                single_indirect = self.alloc_block()?;
                double_buf[offset1..offset1 + 4].copy_from_slice(&single_indirect.to_le_bytes());
                double_updated = true;
                inode.i_blocks += self.block_size / 512;
            }

            let mut indirect_buf = alloc::vec![0u8; self.block_size as usize];
            if double_updated {
                write_blocks(
                    &*self.block_dev,
                    self.block_size,
                    double_indirect,
                    &double_buf,
                )?;
                self.write_inode(inode_num, inode)?;
            } else {
                read_blocks(
                    &*self.block_dev,
                    self.block_size,
                    single_indirect,
                    &mut indirect_buf,
                )?;
            }

            let offset2 = (idx2 * 4) as usize;
            let mut p_block = u32::from_le_bytes([
                indirect_buf[offset2],
                indirect_buf[offset2 + 1],
                indirect_buf[offset2 + 2],
                indirect_buf[offset2 + 3],
            ]);

            if p_block == 0 && alloc {
                p_block = self.alloc_block()?;
                indirect_buf[offset2..offset2 + 4].copy_from_slice(&p_block.to_le_bytes());
                write_blocks(
                    &*self.block_dev,
                    self.block_size,
                    single_indirect,
                    &indirect_buf,
                )?;
                inode.i_blocks += self.block_size / 512;
                self.write_inode(inode_num, inode)?;
            }
            return Ok(p_block);
        }

        Err(Error::InvalidArgs)
    }

    pub fn read_file_data(
        &self,
        inode: &mut Inode,
        inode_num: u32,
        offset: u64,
        buf: &mut [u8],
    ) -> Result<usize> {
        let size = inode.i_size as u64;
        if offset >= size {
            return Ok(0);
        }
        let read_len = core::cmp::min(buf.len() as u64, size - offset) as usize;
        let mut bytes_read = 0;

        while bytes_read < read_len {
            let curr_offset = offset + bytes_read as u64;
            let logical_block = (curr_offset / self.block_size as u64) as u32;
            let offset_in_block = (curr_offset % self.block_size as u64) as usize;
            let chunk_len = core::cmp::min(
                read_len - bytes_read,
                self.block_size as usize - offset_in_block,
            );

            let p_block = self.translate_block(inode, inode_num, logical_block, false)?;
            if p_block == 0 {
                // Sparsely allocated block
                buf[bytes_read..bytes_read + chunk_len].fill(0);
            } else {
                let mut block_buf = alloc::vec![0u8; self.block_size as usize];
                read_blocks(&*self.block_dev, self.block_size, p_block, &mut block_buf)?;
                buf[bytes_read..bytes_read + chunk_len]
                    .copy_from_slice(&block_buf[offset_in_block..offset_in_block + chunk_len]);
            }
            bytes_read += chunk_len;
        }
        Ok(bytes_read)
    }

    pub fn write_file_data(
        &self,
        inode: &mut Inode,
        inode_num: u32,
        offset: u64,
        buf: &[u8],
    ) -> Result<usize> {
        let mut bytes_written = 0;
        let write_len = buf.len();

        while bytes_written < write_len {
            let curr_offset = offset + bytes_written as u64;
            let logical_block = (curr_offset / self.block_size as u64) as u32;
            let offset_in_block = (curr_offset % self.block_size as u64) as usize;
            let chunk_len = core::cmp::min(
                write_len - bytes_written,
                self.block_size as usize - offset_in_block,
            );

            let p_block = self.translate_block(inode, inode_num, logical_block, true)?;
            let mut block_buf = alloc::vec![0u8; self.block_size as usize];
            if chunk_len < self.block_size as usize {
                read_blocks(&*self.block_dev, self.block_size, p_block, &mut block_buf)?;
            }
            block_buf[offset_in_block..offset_in_block + chunk_len]
                .copy_from_slice(&buf[bytes_written..bytes_written + chunk_len]);
            write_blocks(&*self.block_dev, self.block_size, p_block, &block_buf)?;

            bytes_written += chunk_len;
        }

        let new_offset = offset + bytes_written as u64;
        if new_offset > inode.i_size as u64 {
            inode.i_size = new_offset as u32;
            self.write_inode(inode_num, inode)?;
        }
        Ok(bytes_written)
    }

    // Truncate file data (specifically free up indirect blocks and direct blocks when deleting or truncating)
    pub fn truncate_inode_blocks(&self, inode: &mut Inode, inode_num: u32) -> Result<()> {
        let pointers_per_block = self.block_size / 4;

        // Free direct blocks
        for i in 0..12 {
            let block = inode.i_block[i];
            if block != 0 {
                self.free_block(block)?;
                inode.i_block[i] = 0;
            }
        }

        // Free singly indirect blocks
        let single = inode.i_block[12];
        if single != 0 {
            let mut indirect_buf = alloc::vec![0u8; self.block_size as usize];
            read_blocks(&*self.block_dev, self.block_size, single, &mut indirect_buf)?;
            for i in 0..pointers_per_block {
                let offset = (i * 4) as usize;
                let block = u32::from_le_bytes([
                    indirect_buf[offset],
                    indirect_buf[offset + 1],
                    indirect_buf[offset + 2],
                    indirect_buf[offset + 3],
                ]);
                if block != 0 {
                    self.free_block(block)?;
                }
            }
            self.free_block(single)?;
            inode.i_block[12] = 0;
        }

        // Free doubly indirect blocks
        let double = inode.i_block[13];
        if double != 0 {
            let mut double_buf = alloc::vec![0u8; self.block_size as usize];
            read_blocks(&*self.block_dev, self.block_size, double, &mut double_buf)?;
            for i in 0..pointers_per_block {
                let offset1 = (i * 4) as usize;
                let single = u32::from_le_bytes([
                    double_buf[offset1],
                    double_buf[offset1 + 1],
                    double_buf[offset1 + 2],
                    double_buf[offset1 + 3],
                ]);
                if single != 0 {
                    let mut indirect_buf = alloc::vec![0u8; self.block_size as usize];
                    read_blocks(&*self.block_dev, self.block_size, single, &mut indirect_buf)?;
                    for j in 0..pointers_per_block {
                        let offset2 = (j * 4) as usize;
                        let block = u32::from_le_bytes([
                            indirect_buf[offset2],
                            indirect_buf[offset2 + 1],
                            indirect_buf[offset2 + 2],
                            indirect_buf[offset2 + 3],
                        ]);
                        if block != 0 {
                            self.free_block(block)?;
                        }
                    }
                    self.free_block(single)?;
                }
            }
            self.free_block(double)?;
            inode.i_block[13] = 0;
        }

        inode.i_blocks = 0;
        inode.i_size = 0;
        self.write_inode(inode_num, inode)?;
        Ok(())
    }

    pub fn read_directory_entries(&self, inode_num: u32) -> Result<Vec<Ext2FileInfo>> {
        let mut inode = self.read_inode(inode_num)?;
        let mut offset = 0u64;
        let size = inode.i_size as u64;
        let mut entries = Vec::new();

        while offset < size {
            let mut header = [0u8; 8];
            let bytes = self.read_file_data(&mut inode, inode_num, offset, &mut header)?;
            if bytes < 8 {
                break;
            }

            let ino = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
            let rec_len = u16::from_le_bytes([header[4], header[5]]) as u64;
            let name_len = header[6] as usize;
            let file_type = header[7];

            if rec_len == 0 {
                break;
            }

            if ino != 0 {
                let mut name_buf = alloc::vec![0u8; name_len];
                self.read_file_data(&mut inode, inode_num, offset + 8, &mut name_buf)?;
                let name = String::from_utf8_lossy(&name_buf).into_owned();
                let is_dir = file_type == EXT2_FT_DIR;

                entries.push(Ext2FileInfo {
                    name,
                    inode_num: ino,
                    mode: if is_dir { EXT2_S_IFDIR } else { EXT2_S_IFREG },
                    size: 0,
                    is_dir,
                });
            }
            offset += rec_len;
        }
        Ok(entries)
    }

    pub fn add_directory_entry(
        &self,
        dir_inode_num: u32,
        name: &str,
        child_inode_num: u32,
        is_dir: bool,
    ) -> Result<()> {
        let mut inode = self.read_inode(dir_inode_num)?;
        let name_bytes = name.as_bytes();
        let name_len = name_bytes.len();
        let required_len = ((8 + name_len + 3) / 4) * 4;

        let mut offset = 0u64;
        let mut size = inode.i_size as u64;

        while offset < size {
            let mut header = [0u8; 8];
            self.read_file_data(&mut inode, dir_inode_num, offset, &mut header)?;
            let rec_len = u16::from_le_bytes([header[4], header[5]]) as usize;
            let entry_ino = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
            let entry_name_len = header[6] as usize;

            if rec_len == 0 {
                break;
            }

            // Check if we can split this record to append our entry
            if entry_ino != 0 {
                let used_len = ((8 + entry_name_len + 3) / 4) * 4;
                if rec_len >= used_len + required_len {
                    // Update current record length
                    let new_rec_len = used_len;
                    let len_buf = (new_rec_len as u16).to_le_bytes();
                    self.write_file_data(&mut inode, dir_inode_num, offset + 4, &len_buf)?;

                    // Write new entry at offset + used_len
                    let new_offset = offset + used_len as u64;
                    let remaining_rec_len = rec_len - used_len;

                    let mut new_entry = alloc::vec![0u8; remaining_rec_len];
                    new_entry[0..4].copy_from_slice(&child_inode_num.to_le_bytes());
                    new_entry[4..6].copy_from_slice(&(remaining_rec_len as u16).to_le_bytes());
                    new_entry[6] = name_len as u8;
                    new_entry[7] = if is_dir {
                        EXT2_FT_DIR
                    } else {
                        EXT2_FT_REG_FILE
                    };
                    new_entry[8..8 + name_len].copy_from_slice(name_bytes);

                    self.write_file_data(&mut inode, dir_inode_num, new_offset, &new_entry)?;
                    return Ok(());
                }
            } else if rec_len >= required_len {
                // Reuse deleted record
                let mut new_entry = alloc::vec![0u8; rec_len];
                new_entry[0..4].copy_from_slice(&child_inode_num.to_le_bytes());
                new_entry[4..6].copy_from_slice(&(rec_len as u16).to_le_bytes());
                new_entry[6] = name_len as u8;
                new_entry[7] = if is_dir {
                    EXT2_FT_DIR
                } else {
                    EXT2_FT_REG_FILE
                };
                new_entry[8..8 + name_len].copy_from_slice(name_bytes);

                self.write_file_data(&mut inode, dir_inode_num, offset, &new_entry)?;
                return Ok(());
            }

            offset += rec_len as u64;
        }

        // No free space in existing blocks, extend directory by one block
        let p_block = self.translate_block(
            &mut inode,
            dir_inode_num,
            (size / self.block_size as u64) as u32,
            true,
        )?;

        let mut block_buf = alloc::vec![0u8; self.block_size as usize];
        block_buf[0..4].copy_from_slice(&child_inode_num.to_le_bytes());
        block_buf[4..6].copy_from_slice(&(self.block_size as u16).to_le_bytes());
        block_buf[6] = name_len as u8;
        block_buf[7] = if is_dir {
            EXT2_FT_DIR
        } else {
            EXT2_FT_REG_FILE
        };
        block_buf[8..8 + name_len].copy_from_slice(name_bytes);

        write_blocks(&*self.block_dev, self.block_size, p_block, &block_buf)?;

        inode.i_size = (size + self.block_size as u64) as u32;
        self.write_inode(dir_inode_num, &inode)?;
        Ok(())
    }

    pub fn remove_directory_entry(&self, dir_inode_num: u32, name: &str) -> Result<u32> {
        let mut inode = self.read_inode(dir_inode_num)?;
        let mut offset = 0u64;
        let size = inode.i_size as u64;

        let mut prev_offset = None;
        let mut prev_rec_len = 0;

        while offset < size {
            let mut header = [0u8; 8];
            self.read_file_data(&mut inode, dir_inode_num, offset, &mut header)?;
            let ino = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
            let rec_len = u16::from_le_bytes([header[4], header[5]]) as usize;
            let name_len = header[6] as usize;

            if rec_len == 0 {
                break;
            }

            let mut name_buf = alloc::vec![0u8; name_len];
            self.read_file_data(&mut inode, dir_inode_num, offset + 8, &mut name_buf)?;
            let entry_name = String::from_utf8_lossy(&name_buf);

            if ino != 0 && entry_name == name {
                // If it is the first entry in a block, we just set ino to 0.
                // Otherwise, we merge its rec_len into the previous entry.
                if let Some(prev) = prev_offset {
                    let merged_rec_len = prev_rec_len + rec_len;
                    let len_buf = (merged_rec_len as u16).to_le_bytes();
                    self.write_file_data(&mut inode, dir_inode_num, prev + 4, &len_buf)?;
                } else {
                    // First entry: set ino to 0
                    let zero_ino = 0u32.to_le_bytes();
                    self.write_file_data(&mut inode, dir_inode_num, offset, &zero_ino)?;
                }
                return Ok(ino);
            }

            prev_offset = Some(offset);
            prev_rec_len = rec_len;
            offset += rec_len as u64;
        }
        Err(Error::InvalidArgs)
    }
}
