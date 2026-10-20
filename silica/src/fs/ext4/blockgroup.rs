// SPDX-License-Identifier: GPL-2.0

//! Ext4 block group descriptor support (32/64-byte variants and checksums).

use ::alloc::vec;
use ::alloc::vec::Vec;

use crate::api::errno::{Errno, Result};
use crate::fs::ext4::crc32c::{crc16, crc32c};

/// Minimum descriptor size (legacy ext2/ext3).
pub const MIN_DESC_SIZE: usize = 32;
/// Descriptor size for filesystems with the 64-bit feature.
pub const B64_DESC_SIZE: usize = 64;

/// One parsed block group descriptor.
#[derive(Clone, Debug, Default)]
pub struct BgDesc {
    pub block_bitmap: u64,
    pub inode_bitmap: u64,
    pub inode_table: u64,
    pub free_blocks_count: u32,
    pub free_inodes_count: u32,
    pub used_dirs_count: u32,
    pub checksum: u32,
}

/// The on-disk group descriptor table.
pub struct BgTable {
    /// Start block of the (contiguous) descriptor table.
    pub first_desc_block: u64,
    /// Desc size in bytes (32 or >= 64).
    pub desc_size: usize,
    /// Number of descriptors.
    pub count: usize,
    /// Raw table blocks, retained for byte-level checksum patching.
    pub raw_jump: Vec<Vec<u8>>,
    /// Checksum mode.
    pub csum: DescSum,
    /// Filesystem UUID (checksum ingredient).
    pub uuid: [u8; 16],
    /// Prepared CRC-32C seed.
    pub csum_seed: u32,
    /// Set when raw blocks must be rewritten.
    pub dirty: bool,
}

/// Checksum algorithm for group descriptors.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DescSum {
    None,
    /// Legacy `gdt_csum`: CRC-16, stored at offset 30.
    Crc16,
    /// `metadata_csum`: CRC-32C (32-bit at offset 60 for large descs,
    /// truncated 16-bit at offset 30 otherwise).
    Crc32c,
}

impl BgTable {
    /// Sets checksum ingredients after loading.
    pub fn set_csum_ingredients(&mut self, uuid: [u8; 16], seed: u32) {
        self.uuid = uuid;
        self.csum_seed = seed;
    }

    /// Reads and parses the descriptor table from storage via `read_block`.
    pub fn load(
        first_desc_block: u64,
        desc_size: usize,
        count: usize,
        block_size: usize,
        read_block: &dyn Fn(u64, &mut [u8]) -> Result<()>,
        csum: DescSum,
    ) -> Result<Self> {
        let per_block = block_size / desc_size;
        let total_blocks = count.div_ceil(per_block).max(1);
        let mut raw_jump = Vec::with_capacity(total_blocks);
        for i in 0..total_blocks {
            let mut blk = vec![0u8; block_size];
            read_block(first_desc_block + i as u64, &mut blk)?;
            raw_jump.push(blk);
        }
        Ok(Self {
            first_desc_block,
            desc_size,
            count,
            raw_jump,
            csum,
            uuid: [0u8; 16],
            csum_seed: 0,
            dirty: false,
        })
    }

    /// Bytes of descriptor `g` inside the retained raw blocks (mutable).
    fn desc_bytes(&mut self, g: usize) -> Result<&mut [u8]> {
        if g >= self.count {
            return Err(Errno::EINVAL);
        }
        let per_block = self.raw_jump[0].len();
        let start = g * self.desc_size;
        let block_idx = start / per_block;
        let in_block = start % per_block;
        let (_, entry) = self.raw_jump.split_at_mut(block_idx);
        Ok(&mut entry[0][in_block..in_block + self.desc_size])
    }

    /// Parses descriptor `g` from the retained raw bytes.
    pub fn desc(&self, g: usize) -> Result<BgDesc> {
        if g >= self.count {
            return Err(Errno::EINVAL);
        }
        let start = g * self.desc_size;
        let per_block = self.raw_jump[0].len();
        let block_idx = start / per_block;
        let in_block = start % per_block;
        let mut bytes = vec![0u8; self.desc_size];
        // A descriptor never spans blocks (desc_size divides block size).
        bytes.copy_from_slice(&self.raw_jump[block_idx][in_block..in_block + self.desc_size]);
        Ok(Self::parse_desc(&bytes, self.desc_size))
    }

    /// Serializes + checksums a descriptor into its raw slot and returns the
    /// updated `BgDesc`.
    pub fn update_desc(&mut self, g: usize, d: &BgDesc) -> Result<BgDesc> {
        if g >= self.count {
            return Err(Errno::EINVAL);
        }
        let mut local = d.clone();
        self.compute_checksum(&mut local, g, &self.uuid, self.csum_seed);
        let mut bytes = vec![0u8; self.desc_size];
        self.serialize_desc(&local, &mut bytes);
        {
            let slot = self.desc_bytes(g)?;
            slot.copy_from_slice(&bytes);
        }
        Ok(local)
    }

    /// Patches the block-bitmap checksum field of descriptor `g` in place.
    pub fn patch_block_bitmap_csum(&mut self, g: usize, sum: u32, has_64bit: bool) -> Result<()> {
        let slot = self.desc_bytes(g)?;
        let le16 = (sum & 0xFFFF) as u16;
        slot[24..26].copy_from_slice(&le16.to_le_bytes());
        if has_64bit {
            slot[56..58].copy_from_slice(&((sum >> 16) as u16).to_le_bytes());
        }
        Ok(())
    }

    /// Patches the inode-bitmap checksum field of descriptor `g` in place.
    pub fn patch_inode_bitmap_csum(&mut self, g: usize, sum: u32, has_64bit: bool) -> Result<()> {
        let slot = self.desc_bytes(g)?;
        let le16 = (sum & 0xFFFF) as u16;
        slot[26..28].copy_from_slice(&le16.to_le_bytes());
        if has_64bit {
            slot[58..60].copy_from_slice(&((sum >> 16) as u16).to_le_bytes());
        }
        Ok(())
    }

    /// Marks the table dirty; the caller rewrites raw blocks to storage.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Parses a single descriptor.
    pub fn parse_desc(b: &[u8], desc_size: usize) -> BgDesc {
        let copy = |off: usize, n: usize| -> Vec<u8> {
            let mut v = vec![0u8; n];
            if let Some(s) = b.get(off..off + n) {
                v.copy_from_slice(s);
            }
            v
        };
        let le32 =
            |off: usize| -> u32 { u32::from_le_bytes(copy(off, 4).try_into().unwrap_or_default()) };
        let le16 =
            |off: usize| -> u16 { u16::from_le_bytes(copy(off, 2).try_into().unwrap_or_default()) };

        let mut d = BgDesc::default();
        d.block_bitmap = (le32(0) as u64)
            | if desc_size >= B64_DESC_SIZE {
                (le32(32) as u64) << 32
            } else {
                0
            };
        d.inode_bitmap = (le32(4) as u64)
            | if desc_size >= B64_DESC_SIZE {
                (le32(36) as u64) << 32
            } else {
                0
            };
        d.inode_table = (le32(8) as u64)
            | if desc_size >= B64_DESC_SIZE {
                (le32(40) as u64) << 32
            } else {
                0
            };
        d.free_blocks_count = if desc_size >= B64_DESC_SIZE {
            le16(12) as u32 | (le16(44) as u32) << 16
        } else {
            le16(12) as u32
        };
        d.free_inodes_count = if desc_size >= B64_DESC_SIZE {
            le16(14) as u32 | (le16(46) as u32) << 16
        } else {
            le16(14) as u32
        };
        d.used_dirs_count = if desc_size >= B64_DESC_SIZE {
            le16(16) as u32 | (le16(48) as u32) << 16
        } else {
            le16(16) as u32
        };

        // Checksum field comes from the raw descriptor bytes.
        d.checksum = if desc_size >= B64_DESC_SIZE {
            le32(60)
        } else {
            le16(30) as u32
        };
        d
    }

    /// Serializes a descriptor into a byte image of `desc_size` bytes.
    pub fn serialize_desc(&self, d: &BgDesc, out: &mut [u8]) {
        for byte in out.iter_mut() {
            *byte = 0;
        }
        put32(out, 0, d.block_bitmap as u32);
        put32(out, 4, d.inode_bitmap as u32);
        put32(out, 8, d.inode_table as u32);
        put16(out, 12, (d.free_blocks_count & 0xFFFF) as u16);
        put16(out, 14, (d.free_inodes_count & 0xFFFF) as u16);
        put16(out, 16, (d.used_dirs_count & 0xFFFF) as u16);

        if self.desc_size >= B64_DESC_SIZE {
            put32(out, 32, (d.block_bitmap >> 32) as u32);
            put32(out, 36, (d.inode_bitmap >> 32) as u32);
            put32(out, 40, (d.inode_table >> 32) as u32);
            put16(out, 44, (d.free_blocks_count >> 16) as u16);
            put16(out, 46, (d.free_inodes_count >> 16) as u16);
            put16(out, 48, (d.used_dirs_count >> 16) as u16);
            // 32-bit checksum field
            out[60..64].copy_from_slice(&d.checksum.to_le_bytes());
        } else {
            // 16-bit checksum at 30.
            put16(out, 30, (d.checksum & 0xFFFF) as u16);
        }
    }

    /// Recomputes the descriptor checksum with zeroed sum field.
    pub fn compute_checksum(&self, d: &mut BgDesc, group: usize, uuid: &[u8], csum_seed: u32) {
        let mut bytes = vec![0u8; self.desc_size];
        let saved = d.checksum;
        d.checksum = 0;
        self.serialize_desc(d, &mut bytes);
        d.checksum = saved;

        match self.csum {
            DescSum::None => {
                d.checksum = 0;
            }
            DescSum::Crc16 => {
                let mut st = crc16(0xFFFF, uuid);
                let bytes16 = (group as u16).to_le_bytes();
                st = crc16(st, &bytes16);
                st = crc16(st, &bytes);
                d.checksum = st as u32;
            }
            DescSum::Crc32c => {
                let mut st = csum_seed;
                if self.desc_size >= B64_DESC_SIZE {
                    let gbytes = (group as u32).to_le_bytes();
                    st = crc32c(st, &gbytes);
                }
                st = crc32c(st, &bytes);
                if self.desc_size >= B64_DESC_SIZE {
                    d.checksum = st;
                } else {
                    d.checksum = st & 0xFFFF;
                }
            }
        }
    }

    /// Flushes all retained descriptor blocks through `write_block`.
    pub fn flush(&self, write_block: &dyn Fn(u64, &[u8]) -> Result<()>) -> Result<()> {
        for (i, blk) in self.raw_jump.iter().enumerate() {
            write_block(self.first_desc_block + i as u64, blk)?;
        }
        Ok(())
    }
}

fn put32(out: &mut [u8], off: usize, v: u32) {
    out[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

fn put16(out: &mut [u8], off: usize, v: u16) {
    out[off..off + 2].copy_from_slice(&v.to_le_bytes());
}
