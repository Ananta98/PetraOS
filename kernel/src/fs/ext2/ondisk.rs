//! Ext2 On-Disk Inode Structure and Serialization

/// Representation of an on-disk Ext2 Inode (128-byte layout for Rev 0/1).
#[derive(Clone, Debug, Default)]
pub struct Ext2Inode {
    pub mode: u16,
    pub uid: u16,
    pub size: u32,
    pub atime: u32,
    pub ctime: u32,
    pub mtime: u32,
    pub dtime: u32,
    pub gid: u16,
    pub links_count: u16,
    pub blocks: u32,
    pub flags: u32,
    pub block: [u32; 15],
}

impl Ext2Inode {
    /// Parse an Ext2 Inode from a 128-byte raw buffer.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 128 {
            return None;
        }
        let mut block = [0u32; 15];
        for i in 0..15 {
            let offset = 40 + i * 4;
            block[i] = u32::from_le_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]);
        }
        Some(Self {
            mode: u16::from_le_bytes([data[0], data[1]]),
            uid: u16::from_le_bytes([data[2], data[3]]),
            size: u32::from_le_bytes([data[4], data[5], data[6], data[7]]),
            atime: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
            ctime: u32::from_le_bytes([data[12], data[13], data[14], data[15]]),
            mtime: u32::from_le_bytes([data[16], data[17], data[18], data[19]]),
            dtime: u32::from_le_bytes([data[20], data[21], data[22], data[23]]),
            gid: u16::from_le_bytes([data[24], data[25]]),
            links_count: u16::from_le_bytes([data[26], data[27]]),
            blocks: u32::from_le_bytes([data[28], data[29], data[30], data[31]]),
            flags: u32::from_le_bytes([data[32], data[33], data[34], data[35]]),
            block,
        })
    }

    /// Serialize into a 128-byte raw buffer.
    pub fn serialize(&self) -> [u8; 128] {
        let mut data = [0u8; 128];
        data[0..2].copy_from_slice(&self.mode.to_le_bytes());
        data[2..4].copy_from_slice(&self.uid.to_le_bytes());
        data[4..8].copy_from_slice(&self.size.to_le_bytes());
        data[8..12].copy_from_slice(&self.atime.to_le_bytes());
        data[12..16].copy_from_slice(&self.ctime.to_le_bytes());
        data[16..20].copy_from_slice(&self.mtime.to_le_bytes());
        data[20..24].copy_from_slice(&self.dtime.to_le_bytes());
        data[24..26].copy_from_slice(&self.gid.to_le_bytes());
        data[26..28].copy_from_slice(&self.links_count.to_le_bytes());
        data[28..32].copy_from_slice(&self.blocks.to_le_bytes());
        data[32..36].copy_from_slice(&self.flags.to_le_bytes());
        for i in 0..15 {
            let offset = 40 + i * 4;
            data[offset..offset + 4].copy_from_slice(&self.block[i].to_le_bytes());
        }
        data
    }

    /// Return true if this inode represents a directory.
    pub fn is_dir(&self) -> bool {
        (self.mode & 0xF000) == 0x4000
    }

    /// Return true if this inode represents a regular file.
    pub fn is_file(&self) -> bool {
        (self.mode & 0xF000) == 0x8000
    }

    /// Return true if this inode represents a symbolic link.
    pub fn is_symlink(&self) -> bool {
        (self.mode & 0xF000) == 0xA000
    }
}
