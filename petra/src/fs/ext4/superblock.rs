// SPDX-License-Identifier: GPL-2.0

//! Ext4 superblock parsing, feature gating, and state updates.
//!
//! Parses the 1024-byte structure at byte offset 1024 of the device. All ext4
//! extensions (feature sets, 64-bit sizes, descriptor size, checksum seed)
//! are decoded here; mutation helpers patch the raw bytes so every writeback
//! stays checksum-consistent through a single code path.

use ::alloc::vec;
use ::alloc::vec::Vec;

use crate::fs::ext4::crc32c::crc32c;
use crate::uapi::errno::{Errno, Result};

/// Standard ext2/3/4 superblock magic (`0xEF53`).
pub const EXT4_SUPER_MAGIC: u16 = 0xEF53;

/// On-disk "clean" filesystem state.
pub const EXT4_VALID_FS: u16 = 0x0001;

// ---------------------------------------------------------------------------
// Feature flags
// ---------------------------------------------------------------------------

/// Compatible feature bits (`s_feature_compat`).
pub mod compat {
    pub const DIR_INDEX: u32 = 0x0000_0020;
    pub const HAS_JOURNAL: u32 = 0x0000_0004;
}

/// Incompatible feature bits (`s_feature_incompat`).
pub mod incompat {
    pub const JOURNAL_DEV: u32 = 0x0000_0008;
    pub const META_BG: u32 = 0x0000_0010;
    pub const EXTENTS: u32 = 0x0000_0040;
    pub const B64: u32 = 0x0000_0080;
    pub const FLEX_BG: u32 = 0x0000_0200;
    pub const EA_INODE: u32 = 0x0000_0800;
    pub const DIRDATA: u32 = 0x0000_1000;
    pub const CSUM_SEED: u32 = 0x0000_2000;
    pub const LARGEDIR: u32 = 0x0000_4000;
    pub const INLINE_DATA: u32 = 0x0000_8000;
    pub const ENCRYPT: u32 = 0x0001_0000;
    pub const CASEFOLD: u32 = 0x0004_0000;
}

/// Read-only-compatible feature bits (`s_feature_ro_compat`).
pub mod rocompat {
    pub const SPARSE_SUPER: u32 = 0x0000_0001;
    pub const HUGE_FILE: u32 = 0x0000_0002;
    pub const GDT_CSUM: u32 = 0x0000_0010;
    pub const DIR_NLINK: u32 = 0x0000_0020;
    pub const EXTRA_ISIZE: u32 = 0x0000_0040;
    pub const QUOTA: u32 = 0x0000_0100;
    pub const BIGALLOC: u32 = 0x0000_0200;
    pub const METADATA_CSUM: u32 = 0x0000_0400;
    pub const READONLY: u32 = 0x0000_1000;
    pub const PROJECT: u32 = 0x0000_2000;
}

// ---------------------------------------------------------------------------
// Parsed superblock
// ---------------------------------------------------------------------------

/// A parsed ext4 superblock, retaining the raw 1024-byte image for patching.
pub struct SuperBlock {
    /// Raw 1024-byte superblock bytes.
    pub raw: Vec<u8>,

    pub inodes_count: u32,
    pub blocks_count_lo: u32,
    pub r_blocks_count_lo: u32,
    pub free_blocks_lo: u32,
    pub free_inodes_count: u32,
    pub first_data_block: u32,
    pub log_block_size: u32,
    pub blocks_per_group: u32,
    pub inodes_per_group: u32,
    pub mtime: u32,
    pub wtime: u32,
    pub mnt_count: u16,
    pub max_mnt_count: i16,
    pub magic: u16,
    pub state: u16,
    pub errors: u16,
    pub rev_level: u32,
    pub first_ino: u32,
    pub inode_size: u16,
    pub feature_compat: u32,
    pub feature_incompat: u32,
    pub feature_ro_compat: u32,
    pub uuid: [u8; 16],
    /// Inode number of the internally stored journal (0 when external).
    pub journal_inum: u32,
    pub desc_size: u16,
    pub first_meta_bg: u32,
    pub mkfs_time: u32,
    pub blocks_count_hi: u32,
    pub r_blocks_count_hi: u32,
    pub free_blocks_hi: u32,
    pub checksum_seed: u32,
}

impl SuperBlock {
    /// Parses a 1024-byte superblock buffer.
    pub fn parse(buf: &[u8]) -> Result<Self> {
        if buf.len() < 1024 {
            return Err(Errno::EINVAL);
        }
        let le32 = |off: usize| -> u32 {
            u32::from_le_bytes(buf[off..off + 4].try_into().unwrap_or([0; 4]))
        };
        let le16 = |off: usize| -> u16 {
            u16::from_le_bytes(buf[off..off + 2].try_into().unwrap_or([0; 2]))
        };

        let magic = le16(56);
        if magic != EXT4_SUPER_MAGIC {
            return Err(Errno::EINVAL);
        }

        let rev_level = le32(76);
        let (first_ino, inode_size) = if rev_level >= 1 {
            (le32(84), le16(88))
        } else {
            (11, 128)
        };

        let desc_size = if le32(96) & incompat::META_BG != 0
            || le32(100) & rocompat::METADATA_CSUM != 0
            || le32(96) & incompat::B64 != 0
        {
            let d = le16(254);
            if d == 0 { 32 } else { d }
        } else {
            32
        };

        let mut uuid = [0u8; 16];
        uuid.copy_from_slice(&buf[104..120]);

        let sb = Self {
            raw: buf.to_vec(),
            inodes_count: le32(0),
            blocks_count_lo: le32(4),
            r_blocks_count_lo: le32(8),
            free_blocks_lo: le32(12),
            free_inodes_count: le32(16),
            first_data_block: le32(20),
            log_block_size: le32(24),
            blocks_per_group: le32(32),
            inodes_per_group: le32(40),
            mtime: le32(44),
            wtime: le32(48),
            mnt_count: le16(52),
            max_mnt_count: i16::from_le_bytes(buf[54..56].try_into().unwrap_or([0; 2])),
            magic,
            state: le16(58),
            errors: le16(60),
            rev_level,
            first_ino,
            inode_size,
            feature_compat: le32(92),
            feature_incompat: le32(96),
            feature_ro_compat: le32(100),
            uuid,
            journal_inum: le32(224),
            desc_size,
            first_meta_bg: le32(260),
            mkfs_time: le32(264),
            blocks_count_hi: le32(336),
            r_blocks_count_hi: le32(340),
            free_blocks_hi: le32(344),
            checksum_seed: le32(560),
        };

        sb.validate()?;
        Ok(sb)
    }

    /// Feature sanity: reject filesystems this driver cannot serve.
    fn validate(&self) -> Result<()> {
        // Features we cannot honor at all.
        let fatal = incompat::JOURNAL_DEV
            | incompat::INLINE_DATA
            | incompat::ENCRYPT
            | rocompat::BIGALLOC
            | rocompat::READONLY
            | rocompat::PROJECT
            | incompat::META_BG;
        if (self.feature_incompat | self.feature_ro_compat) & fatal != 0 {
            return Err(Errno::EINVAL);
        }
        if self.log_block_size > 3 {
            return Err(Errno::EINVAL);
        }
        if self.inode_size < 128 {
            return Err(Errno::EINVAL);
        }
        Ok(())
    }

    /// Filesystem block size in bytes.
    pub fn block_size(&self) -> usize {
        1024usize << self.log_block_size
    }

    /// Total filesystem size in FS blocks (64-bit).
    pub fn blocks_count(&self) -> u64 {
        (self.blocks_count_lo as u64) | ((self.blocks_count_hi as u64) << 32)
    }

    /// Total free blocks (64-bit).
    pub fn free_blocks_count(&self) -> u64 {
        (self.free_blocks_lo as u64) | ((self.free_blocks_hi as u64) << 32)
    }

    /// Reserved blocks (64-bit).
    pub fn r_blocks_count(&self) -> u64 {
        (self.r_blocks_count_lo as u64) | ((self.r_blocks_count_hi as u64) << 32)
    }

    /// Total number of block groups.
    pub fn block_groups_count(&self) -> u64 {
        let blocks = self.blocks_count() - self.first_data_block as u64;
        if self.blocks_per_group == 0 {
            return 1;
        }
        blocks.div_ceil(self.blocks_per_group as u64)
    }

    /// Whether this filesystem uses extents.
    pub fn uses_extents(&self) -> bool {
        self.feature_incompat & incompat::EXTENTS != 0
    }

    /// Whether metadata checksums are enabled (`metadata_csum`).
    pub fn has_metadata_csum(&self) -> bool {
        self.feature_ro_compat & rocompat::METADATA_CSUM != 0
    }

    /// Whether group descriptors were extended to 64 bits.
    pub fn has_64bit(&self) -> bool {
        self.feature_incompat & incompat::B64 != 0
    }

    /// Whether an embedded journal exists.
    pub fn has_journal(&self) -> bool {
        self.feature_compat & compat::HAS_JOURNAL != 0
    }

    /// Whether directory indexing (dir_index / HTree) is supported.
    pub fn has_dir_index(&self) -> bool {
        self.feature_compat & compat::DIR_INDEX != 0
    }

    /// Default HTree hash version (`s_def_hash_version`, offset 252 / 0xFC).
    pub fn def_hash_version(&self) -> u8 {
        self.raw.get(252).copied().unwrap_or(1)
    }

    /// 128-bit HTree hash seed (`s_hash_seed`, 4x u32 starting at offset 236 / 0xEC).
    pub fn hash_seed(&self) -> [u32; 4] {
        let mut seed = [0u32; 4];
        if self.raw.len() >= 252 {
            for i in 0..4 {
                seed[i] = u32::from_le_bytes(
                    self.raw[236 + i * 4..236 + i * 4 + 4]
                        .try_into()
                        .unwrap_or([0; 4]),
                );
            }
        }
        seed
    }

    /// Checksum seed: explicit `s_checksum_seed`, or derived from the UUID.
    ///
    /// The seed is the CRC-32C register state of the UUID, chained exactly as
    /// the kernel's `s_csum_seed` (no finalized inversion).
    pub fn csum_seed(&self) -> u32 {
        if self.feature_incompat & incompat::CSUM_SEED != 0 && self.checksum_seed != 0 {
            self.checksum_seed
        } else {
            crc32c(0xFFFF_FFFF, &self.uuid)
        }
    }

    /// Recomputes and stores the superblock checksum inside `raw`.
    pub fn recompute_sb_checksum(&mut self) {
        if self.has_metadata_csum() {
            let seed = self.csum_seed();
            let sum = crc32c(seed, &self.raw[0..1020]);
            self.raw[1020..1024].copy_from_slice(&sum.to_le_bytes());
        }
    }

    #[inline]
    fn patch32(&mut self, off: usize, val: u32) {
        self.raw[off..off + 4].copy_from_slice(&val.to_le_bytes());
    }

    #[inline]
    fn patch16(&mut self, off: usize, val: u16) {
        self.raw[off..off + 2].copy_from_slice(&val.to_le_bytes());
    }

    /// Updates free-block counters (keeping 64-bit hi/lo in sync).
    pub fn set_free_blocks(&mut self, v: u64) {
        self.free_blocks_lo = v as u32;
        self.free_blocks_hi = (v >> 32) as u32;
        self.patch32(12, self.free_blocks_lo);
        self.patch32(344, self.free_blocks_hi);
        self.recompute_sb_checksum();
    }

    /// Updates the free-inode counter.
    pub fn set_free_inodes(&mut self, v: u32) {
        self.free_inodes_count = v;
        self.patch32(16, v);
        self.recompute_sb_checksum();
    }

    /// Marks the filesystem state (e.g. `EXT4_VALID_FS`).
    pub fn set_state(&mut self, state: u16) {
        self.state = state;
        self.patch16(58, state);
        self.recompute_sb_checksum();
    }

    /// Directly patches the free-inodes counter (allocator's bookkeeping).
    pub fn patch_free_inodes(&mut self, v: u32) {
        self.free_inodes_count = v;
        self.patch32(16, v);
        self.recompute_sb_checksum();
    }

    /// Directly patches the free-blocks counter (allocator's bookkeeping).
    pub fn patch_free_blocks(&mut self, v: u64) {
        self.free_blocks_lo = v as u32;
        self.free_blocks_hi = (v >> 32) as u32;
        self.patch32(12, self.free_blocks_lo);
        self.patch32(344, self.free_blocks_hi);
        self.recompute_sb_checksum();
    }

    /// Bumps the mount counter on the raw image.
    pub fn record_mount(&mut self) {
        self.mnt_count = self.mnt_count.saturating_add(1);
        self.wtime = super::inode::nowsecs32();
        self.patch16(52, self.mnt_count);
        self.patch32(48, self.wtime);
        self.recompute_sb_checksum();
    }

    fn debug_validate_free(&self, _v: u64) {
        // Placeholder for future invariant checks; kept out of hot paths.
    }
}

/// Generates a fresh zeroed buffer of `size` bytes.
pub fn zero_buffer(size: usize) -> Vec<u8> {
    vec![0u8; size]
}
