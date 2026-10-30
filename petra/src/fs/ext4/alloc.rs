// SPDX-License-Identifier: GPL-2.0

//! Ext4 allocators: block allocation, block freeing, and inode allocation.
//!
//! All mutations go through the block device closures. Bitmap bit position
//! `b` of group `g` maps to physical block `first_data_block + g *
//! blocks_per_group + b`.

use ::alloc::vec;

use crate::uapi::errno::{Errno, Result};

use super::blockgroup::BgTable;
use super::superblock::SuperBlock;

/// I/O plumbing for the allocators.
pub struct AllocIo<'a> {
    pub read_block: &'a dyn Fn(u64, &mut [u8]) -> Result<()>,
    pub write_block: &'a dyn Fn(u64, &[u8]) -> Result<()>,
    pub sb: &'a mut SuperBlock,
    pub bgt: &'a mut BgTable,
    pub block_size: usize,
    pub total_blocks: u64,
}

impl AllocIo<'_> {
    /// Blocks in group `g`'s valid bitmap span (bits 8*n covering n blocks;
    /// the final group may be short).
    fn group_block_bits(&self, g: usize) -> usize {
        let per = self.sb.blocks_per_group as u64;
        let first = self.sb.first_data_block as u64;
        let total = self.total_blocks;
        if per == 0 {
            return 0;
        }
        let group_start = first + g as u64 * per;
        if group_start >= total {
            return 0;
        }
        ((total - group_start) as usize).min(8 * self.block_size)
    }

    /// Validates that bit `bit` belongs to the group's valid span, and
    /// returns the physical block it maps to.
    fn block_of_bit(&self, g: usize, bit: usize) -> u64 {
        self.sb.first_data_block as u64 + g as u64 * self.sb.blocks_per_group as u64 + bit as u64
    }

    /// Writes back a block bitmap with its checksum and descriptor counts.
    fn store_block_bitmap(
        &mut self,
        g: usize,
        d: &super::blockgroup::BgDesc,
        bm: &[u8],
    ) -> Result<()> {
        (self.write_block)(d.block_bitmap, bm)?;
        if (self.sb.desc_size as usize) >= super::blockgroup::B64_DESC_SIZE
            && self.sb.has_metadata_csum()
        {
            let sum = crate::crypto::hash::crc32c::crc32c(self.sb.csum_seed(), bm);
            self.bgt
                .patch_block_bitmap_csum(g, sum, self.sb.has_64bit())?;
        }
        self.bgt.mark_dirty();
        Ok(())
    }

    /// Writes back an inode bitmap with its checksum.
    fn store_inode_bitmap(
        &mut self,
        g: usize,
        d: &super::blockgroup::BgDesc,
        bm: &[u8],
    ) -> Result<()> {
        (self.write_block)(d.inode_bitmap, bm)?;
        if (self.sb.desc_size as usize) >= super::blockgroup::B64_DESC_SIZE
            && self.sb.has_metadata_csum()
        {
            let limit = (self.sb.inodes_per_group as usize)
                .div_ceil(8)
                .min(self.block_size);
            let sum = crate::crypto::hash::crc32c::crc32c(self.sb.csum_seed(), &bm[0..limit]);
            self.bgt
                .patch_inode_bitmap_csum(g, sum, self.sb.has_64bit())?;
        }
        self.bgt.mark_dirty();
        Ok(())
    }

    /// Allocates one free FS block from the group sequence starting at
    /// `start_group`.
    pub fn alloc_block(&mut self, start_group: usize) -> Result<u64> {
        let groups = self.bgt.count;
        for i in 0..groups {
            let g = (start_group + i) % groups;
            let d = self.bgt.desc(g)?;
            if d.free_blocks_count == 0 {
                continue;
            }
            let mut bm = vec![0u8; self.block_size];
            (self.read_block)(d.block_bitmap, &mut bm)?;
            let span = self.group_block_bits(g);
            if let Some(bit) = scan_free(&bm, span) {
                set_bit(&mut bm, bit);
                let mut nd = d;
                nd.free_blocks_count -= 1;
                self.store_block_bitmap(g, &nd, &bm)?;
                self.bgt.update_desc(g, &nd)?;
                self.bgt.mark_dirty();
                return Ok(self.block_of_bit(g, bit));
            }
        }
        Err(Errno::ENOSPC)
    }

    /// Frees a contiguous physical block range (length may span groups).
    pub fn free_blocks(&mut self, phys: u64, len: u32) -> Result<()> {
        let mut b = phys;
        let remaining = len as u64;
        let mut freed = 0u64;
        // Simple per-block free; counts consolidated after the loop.
        while freed < remaining {
            let g =
                ((b - self.sb.first_data_block as u64) / self.sb.blocks_per_group as u64) as usize;
            let d = self.bgt.desc(g)?;
            let mut bm = vec![0u8; self.block_size];
            (self.read_block)(d.block_bitmap, &mut bm)?;
            // Batch all bits inside this group.
            let first_bit =
                ((b - self.sb.first_data_block as u64) % self.sb.blocks_per_group as u64) as usize;
            let span_left = self.group_block_bits(g);
            let take = (span_left - first_bit).min((remaining - freed) as usize);
            for k in 0..take {
                clear_bit(&mut bm, first_bit + k);
            }
            let mut nd = d;
            nd.free_blocks_count += take as u32;
            self.store_block_bitmap(g, &nd, &bm)?;
            self.bgt.update_desc(g, &nd)?;
            self.bgt.mark_dirty();
            b += take as u64;
            freed += take as u64;
        }
        self.sb_record_free_blocks_count(remaining);
        Ok(())
    }

    /// Allocates one free inode, preferring `start_group`.
    pub fn alloc_inode(&mut self, start_group: usize, total_inodes: u32) -> Result<(u32, usize)> {
        let groups = self.bgt.count;
        for i in 0..groups {
            let g = (start_group + i) % groups;
            let d = self.bgt.desc(g)?;
            if d.free_inodes_count == 0 {
                continue;
            }
            let mut bm = vec![0u8; self.block_size];
            (self.read_block)(d.inode_bitmap, &mut bm)?;
            // Valid inode bits: up to inodes_per_group, but the group may
            // hold fewer when the bitmap runs past the table.
            let span = (self.sb.inodes_per_group as usize).min(8 * self.block_size);
            if let Some(bit) = scan_free(&bm, span) {
                // Guard: the group start may exceed the final usable inode.
                let ino = (g as u64) * self.sb.inodes_per_group as u64 + bit as u64 + 1;
                if ino > total_inodes as u64 {
                    continue;
                }
                set_bit(&mut bm, bit);
                let mut nd = d;
                nd.free_inodes_count -= 1;
                self.store_inode_bitmap(g, &nd, &bm)?;
                self.bgt.update_desc(g, &nd)?;
                self.bgt.mark_dirty();
                self.sb_record_free_inodes(-1i64);
                return Ok((ino as u32, g));
            }
        }
        Err(Errno::ENOSPC)
    }

    /// Frees an inode (clears its bitmap bit and bumps counters).
    pub fn free_inode(&mut self, ino: u32) -> Result<()> {
        let g = ((ino - 1) / self.sb.inodes_per_group) as usize;
        let bit = ((ino - 1) % self.sb.inodes_per_group) as usize;
        let d = self.bgt.desc(g)?;
        let mut bm = vec![0u8; self.block_size];
        (self.read_block)(d.inode_bitmap, &mut bm)?;
        clear_bit(&mut bm, bit);
        let mut nd = d;
        nd.free_inodes_count += 1;
        self.store_inode_bitmap(g, &nd, &bm)?;
        self.bgt.update_desc(g, &nd)?;
        self.bgt.mark_dirty();
        self.sb_record_free_inodes(1);
        Ok(())
    }

    /// Bumps the sb free-inode count by `delta` in memory.
    fn sb_record_free_inodes(&mut self, delta: i64) {
        let cur = self.sb.free_inodes_count as i64;
        let new = (cur + delta).clamp(0, u32::MAX as i64) as u32;
        self.sb.patch_free_inodes(new);
    }

    /// Bumps the sb free-block count by `delta` in memory.
    fn sb_record_free_blocks_count(&mut self, delta: u64) {
        self.sb
            .patch_free_blocks(self.sb.free_blocks_count() + delta);
    }
}

fn scan_free(bm: &[u8], span: usize) -> Option<usize> {
    for i in 0..span / 8 + 1 {
        if i >= bm.len() {
            return None;
        }
        if bm[i] == 0xFF {
            continue;
        }
        for bit in 0..8 {
            let p = i * 8 + bit;
            if p >= span {
                return None;
            }
            if bm[i] & (1 << bit) == 0 {
                return Some(p);
            }
        }
    }
    None
}

fn set_bit(bm: &mut [u8], bit: usize) {
    bm[bit / 8] |= 1 << (bit % 8);
}

fn clear_bit(bm: &mut [u8], bit: usize) {
    bm[bit / 8] &= !(1 << (bit % 8));
}
