/// Ext2 Superblock parsing and structure definitions.
///
/// Designed to be safe, clean, and not rely on raw pointer castings.

#[derive(Clone, Debug)]
pub struct Ext2Superblock {
    pub inodes_count: u32,
    pub blocks_count: u32,
    pub free_blocks_count: u32,
    pub free_inodes_count: u32,
    pub first_data_block: u32,
    pub log_block_size: u32,
    pub blocks_per_group: u32,
    pub inodes_per_group: u32,
    pub magic: u16,
    pub rev_level: u32,
    pub inode_size: u16,
    pub block_size: u32,
}

impl Ext2Superblock {
    /// Parse an Ext2 superblock from a raw byte slice of 1024 bytes.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 84 {
            return None;
        }

        // Validate Ext2 Magic Number
        let magic = u16::from_le_bytes([data[56], data[57]]);
        if magic != 0xEF53 {
            return None;
        }

        let log_block_size = u32::from_le_bytes([data[24], data[25], data[26], data[27]]);
        let block_size = 1024 << log_block_size;

        let rev_level = u32::from_le_bytes([data[76], data[77], data[78], data[79]]);
        let inode_size = if rev_level >= 1 && data.len() >= 90 {
            u16::from_le_bytes([data[88], data[89]])
        } else {
            128 // Default inode size for revision 0
        };

        Some(Self {
            inodes_count: u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
            blocks_count: u32::from_le_bytes([data[4], data[5], data[6], data[7]]),
            free_blocks_count: u32::from_le_bytes([data[12], data[13], data[14], data[15]]),
            free_inodes_count: u32::from_le_bytes([data[16], data[17], data[18], data[19]]),
            first_data_block: u32::from_le_bytes([data[20], data[21], data[22], data[23]]),
            log_block_size,
            blocks_per_group: u32::from_le_bytes([data[32], data[33], data[34], data[35]]),
            inodes_per_group: u32::from_le_bytes([data[40], data[41], data[42], data[43]]),
            magic,
            rev_level,
            inode_size,
            block_size,
        })
    }
}

#[derive(Clone, Debug)]
pub struct Ext2BlockGroupDescriptor {
    pub block_bitmap: u32,
    pub inode_bitmap: u32,
    pub inode_table: u32,
    pub free_blocks_count: u16,
    pub free_inodes_count: u16,
    pub used_dirs_count: u16,
}

impl Ext2BlockGroupDescriptor {
    /// Parse a 32-byte block group descriptor.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 32 {
            return None;
        }

        Some(Self {
            block_bitmap: u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
            inode_bitmap: u32::from_le_bytes([data[4], data[5], data[6], data[7]]),
            inode_table: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
            free_blocks_count: u16::from_le_bytes([data[12], data[13]]),
            free_inodes_count: u16::from_le_bytes([data[14], data[15]]),
            used_dirs_count: u16::from_le_bytes([data[16], data[17]]),
        })
    }
}
