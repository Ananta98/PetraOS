/// EXT2 Filesystem Structs
///
/// Defines the on-disk format structures of EXT2 (Superblock, GroupDescriptor, Inode, etc.)
/// and basic configuration constants.

pub const EXT2_MAGIC: u16 = 0xEF53;

// Inode mode flags
pub const EXT2_S_IFDIR: u16 = 0x4000;
pub const EXT2_S_IFREG: u16 = 0x8000;

// Directory file types (revision >= 1)
pub const EXT2_FT_REG_FILE: u8 = 1;
pub const EXT2_FT_DIR: u8 = 2;

#[derive(Debug, Clone)]
pub struct Superblock {
    pub s_inodes_count: u32,
    pub s_blocks_count: u32,
    pub s_r_blocks_count: u32,
    pub s_free_blocks_count: u32,
    pub s_free_inodes_count: u32,
    pub s_first_data_block: u32,
    pub s_log_block_size: u32,
    pub s_log_frag_size: u32,
    pub s_blocks_per_group: u32,
    pub s_frags_per_group: u32,
    pub s_inodes_per_group: u32,
    pub s_magic: u16,
    pub s_state: u16,
    pub s_errors: u16,
    pub s_minor_rev_level: u16,
    pub s_rev_level: u32,
    pub s_first_ino: u32,
    pub s_inode_size: u16,
}

impl Superblock {
    pub fn block_size(&self) -> u32 {
        1024 << self.s_log_block_size
    }

    pub fn parse(buf: &[u8]) -> Self {
        Self {
            s_inodes_count: u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]),
            s_blocks_count: u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]),
            s_r_blocks_count: u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]),
            s_free_blocks_count: u32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]]),
            s_free_inodes_count: u32::from_le_bytes([buf[16], buf[17], buf[18], buf[19]]),
            s_first_data_block: u32::from_le_bytes([buf[20], buf[21], buf[22], buf[23]]),
            s_log_block_size: u32::from_le_bytes([buf[24], buf[25], buf[26], buf[27]]),
            s_log_frag_size: u32::from_le_bytes([buf[28], buf[29], buf[30], buf[31]]),
            s_blocks_per_group: u32::from_le_bytes([buf[32], buf[33], buf[34], buf[35]]),
            s_frags_per_group: u32::from_le_bytes([buf[36], buf[37], buf[38], buf[39]]),
            s_inodes_per_group: u32::from_le_bytes([buf[40], buf[41], buf[42], buf[43]]),
            s_magic: u16::from_le_bytes([buf[56], buf[57]]),
            s_state: u16::from_le_bytes([buf[58], buf[59]]),
            s_errors: u16::from_le_bytes([buf[60], buf[61]]),
            s_minor_rev_level: u16::from_le_bytes([buf[62], buf[63]]),
            s_rev_level: u32::from_le_bytes([buf[76], buf[77], buf[78], buf[79]]),
            s_first_ino: u32::from_le_bytes([buf[84], buf[85], buf[86], buf[87]]),
            s_inode_size: u16::from_le_bytes([buf[88], buf[89]]),
        }
    }

    pub fn serialize(&self, buf: &mut [u8]) {
        buf[0..4].copy_from_slice(&self.s_inodes_count.to_le_bytes());
        buf[4..8].copy_from_slice(&self.s_blocks_count.to_le_bytes());
        buf[8..12].copy_from_slice(&self.s_r_blocks_count.to_le_bytes());
        buf[12..16].copy_from_slice(&self.s_free_blocks_count.to_le_bytes());
        buf[16..20].copy_from_slice(&self.s_free_inodes_count.to_le_bytes());
        buf[20..24].copy_from_slice(&self.s_first_data_block.to_le_bytes());
        buf[24..28].copy_from_slice(&self.s_log_block_size.to_le_bytes());
        buf[28..32].copy_from_slice(&self.s_log_frag_size.to_le_bytes());
        buf[32..36].copy_from_slice(&self.s_blocks_per_group.to_le_bytes());
        buf[36..40].copy_from_slice(&self.s_frags_per_group.to_le_bytes());
        buf[40..44].copy_from_slice(&self.s_inodes_per_group.to_le_bytes());
        buf[56..58].copy_from_slice(&self.s_magic.to_le_bytes());
        buf[58..60].copy_from_slice(&self.s_state.to_le_bytes());
        buf[60..62].copy_from_slice(&self.s_errors.to_le_bytes());
        buf[62..64].copy_from_slice(&self.s_minor_rev_level.to_le_bytes());
        buf[76..80].copy_from_slice(&self.s_rev_level.to_le_bytes());
        buf[84..88].copy_from_slice(&self.s_first_ino.to_le_bytes());
        buf[88..90].copy_from_slice(&self.s_inode_size.to_le_bytes());
    }
}

#[derive(Debug, Clone)]
pub struct GroupDescriptor {
    pub bg_block_bitmap: u32,
    pub bg_inode_bitmap: u32,
    pub bg_inode_table: u32,
    pub bg_free_blocks_count: u16,
    pub bg_free_inodes_count: u16,
    pub bg_used_dirs_count: u16,
}

impl GroupDescriptor {
    pub fn parse(buf: &[u8]) -> Self {
        Self {
            bg_block_bitmap: u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]),
            bg_inode_bitmap: u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]),
            bg_inode_table: u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]),
            bg_free_blocks_count: u16::from_le_bytes([buf[12], buf[13]]),
            bg_free_inodes_count: u16::from_le_bytes([buf[14], buf[15]]),
            bg_used_dirs_count: u16::from_le_bytes([buf[16], buf[17]]),
        }
    }

    pub fn serialize(&self, buf: &mut [u8]) {
        buf[0..4].copy_from_slice(&self.bg_block_bitmap.to_le_bytes());
        buf[4..8].copy_from_slice(&self.bg_inode_bitmap.to_le_bytes());
        buf[8..12].copy_from_slice(&self.bg_inode_table.to_le_bytes());
        buf[12..14].copy_from_slice(&self.bg_free_blocks_count.to_le_bytes());
        buf[14..16].copy_from_slice(&self.bg_free_inodes_count.to_le_bytes());
        buf[16..18].copy_from_slice(&self.bg_used_dirs_count.to_le_bytes());
    }
}

#[derive(Debug, Clone)]
pub struct Inode {
    pub i_mode: u16,
    pub i_uid: u16,
    pub i_size: u32,
    pub i_atime: u32,
    pub i_ctime: u32,
    pub i_mtime: u32,
    pub i_dtime: u32,
    pub i_gid: u16,
    pub i_links_count: u16,
    pub i_blocks: u32, // 512-byte sectors count
    pub i_flags: u32,
    pub i_block: [u32; 15],
}

impl Inode {
    pub fn parse(buf: &[u8]) -> Self {
        let mut i_block = [0u32; 15];
        for i in 0..15 {
            let offset = 40 + i * 4;
            i_block[i] = u32::from_le_bytes([
                buf[offset],
                buf[offset + 1],
                buf[offset + 2],
                buf[offset + 3],
            ]);
        }
        Self {
            i_mode: u16::from_le_bytes([buf[0], buf[1]]),
            i_uid: u16::from_le_bytes([buf[2], buf[3]]),
            i_size: u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]),
            i_atime: u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]),
            i_ctime: u32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]]),
            i_mtime: u32::from_le_bytes([buf[16], buf[17], buf[18], buf[19]]),
            i_dtime: u32::from_le_bytes([buf[20], buf[21], buf[22], buf[23]]),
            i_gid: u16::from_le_bytes([buf[24], buf[25]]),
            i_links_count: u16::from_le_bytes([buf[26], buf[27]]),
            i_blocks: u32::from_le_bytes([buf[28], buf[29], buf[30], buf[31]]),
            i_flags: u32::from_le_bytes([buf[32], buf[33], buf[34], buf[35]]),
            i_block,
        }
    }

    pub fn serialize(&self, buf: &mut [u8]) {
        buf[0..2].copy_from_slice(&self.i_mode.to_le_bytes());
        buf[2..4].copy_from_slice(&self.i_uid.to_le_bytes());
        buf[4..8].copy_from_slice(&self.i_size.to_le_bytes());
        buf[8..12].copy_from_slice(&self.i_atime.to_le_bytes());
        buf[12..16].copy_from_slice(&self.i_ctime.to_le_bytes());
        buf[16..20].copy_from_slice(&self.i_mtime.to_le_bytes());
        buf[20..24].copy_from_slice(&self.i_dtime.to_le_bytes());
        buf[24..26].copy_from_slice(&self.i_gid.to_le_bytes());
        buf[26..28].copy_from_slice(&self.i_links_count.to_le_bytes());
        buf[28..32].copy_from_slice(&self.i_blocks.to_le_bytes());
        buf[32..36].copy_from_slice(&self.i_flags.to_le_bytes());
        for i in 0..15 {
            let offset = 40 + i * 4;
            buf[offset..offset + 4].copy_from_slice(&self.i_block[i].to_le_bytes());
        }
    }
}
