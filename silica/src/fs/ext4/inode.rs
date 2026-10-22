// SPDX-License-Identifier: GPL-2.0

//! Ext4 on-disk inode structure (both 128-byte legacy and 256-byte with
//! extended fields) plus the per-inode checksum helper.

use ::alloc::vec::Vec;

use crate::api::errno::{Errno, Result};
use crate::fs::ext4::crc32c::crc32c;

/// Root inode number for the filesystem root directory.
pub const ROOT_INO: u32 = 2;
/// Inode number of the lost+found directory.
pub const LOST_FOUND_INO: u32 = 11;
/// Inode number typically holding the number-reserved file entry.
pub const RESIZE_INO: u32 = 7;
/// The journal is normally stored in this inode.
pub const JOURNAL_INO: u32 = 8;
/// The electronic dynamic superblock usually lives here.
pub const DYNAMIC_INO: u32 = 6;

/// Inode flags (`i_flags`).
pub mod iflags {
    /// Inode stores an extent tree instead of block pointers.
    pub const EXTENTS: u32 = 0x0008_0000;
    /// Size is encoded in blocks (66-bit "huge file" sizes).
    pub const HUGE_FILE: u32 = 0x0004_0000;
    /// inode is an inline-data container.
    pub const INLINE_DATA: u32 = 0x1000_0000;
}

/// Time-of-day helper: seconds since boot, as stored on disk.
pub fn nowsecs32() -> u32 {
    crate::time::uptime_secs() as u32
}

/// A parsed on-disk inode; retains the full raw bytes for round-tripping.
#[derive(Debug)]
pub struct RawInode {
    /// Raw bytes (exactly `inode_size`), including tail-order fields.
    pub raw: Vec<u8>,

    pub mode: u16,
    pub uid: u32,
    pub gid: u32,
    pub size_lo: u32,
    pub atime: u32,
    pub ctime: u32,
    pub mtime: u32,
    pub dtime: u32,
    pub links_count: u16,
    pub blocks_lo: u32,
    pub blocks_hi: u16,
    pub flags: u32,
    pub osd1: u32,
    pub block: [u32; 15],
    pub generation: u32,
    pub file_acl_lo: u32,
}

impl RawInode {
    /// Parses a raw inode from exactly `inode_size` bytes read from the table.
    pub fn parse(buf: &[u8], inode_size: usize) -> Result<Self> {
        if buf.len() < 128 || inode_size < 128 || buf.len() < inode_size {
            return Err(Errno::EINVAL);
        }
        let le32 = |off: usize| -> u32 {
            u32::from_le_bytes(buf[off..off + 4].try_into().unwrap_or([0; 4]))
        };
        let le16 = |off: usize| -> u16 {
            u16::from_le_bytes(buf[off..off + 2].try_into().unwrap_or([0; 2]))
        };

        let mut block = [0u32; 15];
        for (i, b) in block.iter_mut().enumerate() {
            *b = le32(40 + i * 4);
        }

        let blocks_hi = if inode_size >= 128 { le16(116) } else { 0 };

        Ok(Self {
            raw: buf[..inode_size].to_vec(),
            mode: le16(0),
            uid: le32_uid(buf, le16(2), inode_size, 120),
            size_lo: le32(4),
            atime: le32(8),
            ctime: le32(12),
            mtime: le32(16),
            dtime: le32(20),
            gid: le32_gid(buf, le16(24), inode_size, 122),
            links_count: le16(26),
            blocks_lo: le32(28),
            blocks_hi,
            flags: le32(32),
            osd1: le32(36),
            block,
            generation: le32(100),
            file_acl_lo: le32(104),
        })
    }

    /// 64-bit file size in bytes (or blocks, for huge-flag files).
    pub fn size(&self) -> u64 {
        let mut s = self.size_lo as u64;
        if self.inode_size() >= 256 {
            s |= (u32::from_le_bytes(self.raw[108..112].try_into().unwrap_or([0; 4])) as u64) << 32;
        }
        if self.flags & iflags::HUGE_FILE != 0 && self.is_regular() {
            s <<= 9;
        }
        s
    }

    /// Updates the 64-bit file size, honoring the huge-file encoding.
    pub fn set_size(&mut self, v: u64) {
        if self.flags & iflags::HUGE_FILE != 0 && self.is_regular() {
            let blocks = v >> 9;
            self.size_lo = blocks as u32;
            if self.inode_size() >= 256 {
                self.raw[108..112].copy_from_slice(&((blocks >> 32) as u32).to_le_bytes());
            }
        } else {
            self.size_lo = v as u32;
            if self.inode_size() >= 256 {
                self.raw[108..112].copy_from_slice(&((v >> 32) as u32).to_le_bytes());
            }
        }
        self.raw[4..8].copy_from_slice(&self.size_lo.to_le_bytes());
    }

    /// On-disk 64-bit `i_blocks` (in 512-byte units).
    pub fn blocks(&self) -> u64 {
        self.blocks_lo as u64 | ((self.blocks_hi as u64) << 32)
    }

    /// Updates the on-disk 64-bit `i_blocks` field.
    pub fn set_blocks(&mut self, v: u64) {
        self.blocks_lo = v as u32;
        self.raw[28..32].copy_from_slice(&self.blocks_lo.to_le_bytes());
        if self.inode_size() >= 256 {
            self.blocks_hi = (v >> 32) as u16;
            self.raw[116..118].copy_from_slice(&self.blocks_hi.to_le_bytes());
        }
    }

    /// Adds `n` allocated 512-byte units to `i_blocks`.
    pub fn add_blocks(&mut self, n: u64) {
        self.set_blocks(self.blocks().saturating_add(n));
    }

    /// Subtracts `n` allocated 512-byte units from `i_blocks`.
    pub fn sub_blocks(&mut self, n: u64) {
        self.set_blocks(self.blocks().saturating_sub(n));
    }

    /// The inode size this was parsed with (bounds on tail fields).
    fn inode_size(&self) -> usize {
        self.raw.len()
    }

    /// Whether this inode holds the extended attribute area.
    pub fn has_extra_isize(&self) -> bool {
        self.inode_size() >= 256
    }

    // -------------------------------------------------------------------
    // Type predicates (mode bits, big-endian style like ext2/ext4)
    // -------------------------------------------------------------------

    fn blk_type(&self) -> u32 {
        (self.mode & 0xF000) as u32
    }

    pub fn is_dir(&self) -> bool {
        self.blk_type() == 0x4000
    }

    pub fn is_regular(&self) -> bool {
        self.blk_type() == 0x8000
    }

    pub fn is_symlink(&self) -> bool {
        self.blk_type() == 0xA000
    }

    pub fn is_chardev(&self) -> bool {
        self.blk_type() == 0x2000
    }

    pub fn is_blockdev(&self) -> bool {
        self.blk_type() == 0x6000
    }

    /// Recomputes nothing (hook point for future per-inode csum control).
    pub fn csum_radio(&self) -> [u8; 4] {
        [0u8; 4]
    }
}

/// Combined 32-bit UID from the 16-bit low + 16-bit hi field.
fn le32_uid(buf: &[u8], low: u16, inode_size: usize, hi_off: usize) -> u32 {
    if inode_size >= 256 {
        let hi = u16::from_le_bytes(
            buf.get(hi_off..hi_off + 2)
                .map_or([0, 0], |s| s.try_into().unwrap_or([0; 2])),
        );
        (hi as u32) << 16 | low as u32
    } else {
        low as u32
    }
}

/// Combined 32-bit GID from the 16-bit low + 16-bit hi field.
fn le32_gid(buf: &[u8], low: u16, inode_size: usize, hi_off: usize) -> u32 {
    le32_uid(buf, low, inode_size, hi_off)
}

/// Recomputes the per-inode checksum into the raw bytes.
///
/// Format (per spec): UUID-seeded CRC-32C chained over `LE32(ino)`,
/// `LE32(generation)` and the raw inode bytes with both 16-bit checksum
/// fields zeroed. The 32-bit result is truncated to its lower 16 bits and
/// stored at 128+2 (i_checksum_hi) plus offset 124 (i_checksum_lo).
pub fn recompute_inode_csum(raw: &mut [u8], seed: u32, ino: u32, inode_gen: u32) {
    if raw.len() < 256 {
        return;
    }
    let orig_hi = u16::from_le_bytes(raw[130..132].try_into().unwrap_or([0; 2]));
    let orig_lo = u16::from_le_bytes(raw[124..126].try_into().unwrap_or([0; 2]));

    raw[124..126].copy_from_slice(&[0, 0]);
    raw[130..132].copy_from_slice(&[0, 0]);

    let mut state = seed;
    state = crc32c(state, &ino.to_le_bytes());
    state = crc32c(state, &inode_gen.to_le_bytes());
    state = crc32c(state, raw);

    // Truncate to 16 bits: lower half goes at 124..126, upper at 130..132.
    let sum_le_bytes = state.to_le_bytes();
    raw[124..126].copy_from_slice(&sum_le_bytes[0..2]);
    raw[130..132].copy_from_slice(&sum_le_bytes[2..4]);

    let _ = (orig_hi, orig_lo);
}
