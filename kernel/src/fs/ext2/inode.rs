use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::string::String;
use crate::fs::errno::VfsError;
use crate::fs::vfs::inode::{Inode, InodeOps, InodeType};
use crate::fs::vfs::file::FileOps;
use crate::device::DEVICE_MANAGER;
use super::superblock::{Ext2Superblock, Ext2BlockGroupDescriptor};
use super::file::Ext2FileOps;

/// Helper to read/write arbitrary bytes from/to a named block device.
#[derive(Clone, Debug)]
pub struct BlockDeviceReader {
    pub device_name: &'static str,
}

impl BlockDeviceReader {
    pub fn new(device_name: &'static str) -> Self {
        Self { device_name }
    }

    /// Read up to `buf.len()` bytes starting at `offset` (in bytes).
    pub fn read_bytes(&self, offset: u64, buf: &mut [u8]) -> Result<(), VfsError> {
        let dm = DEVICE_MANAGER.lock();
        for dev_arc in dm.get_devices() {
            let mut dev_lock = dev_arc.lock();
            if dev_lock.name() == self.device_name {
                if let Some(block_dev) = dev_lock.as_block_device_mut() {
                    let sector_size = block_dev.block_size() as u64;
                    let mut read_offset = offset;
                    let mut buf_offset = 0;
                    let mut sector_buf = alloc::vec![0u8; sector_size as usize];

                    while buf_offset < buf.len() {
                        let sector_id = read_offset / sector_size;
                        let sector_offset = (read_offset % sector_size) as usize;
                        
                        block_dev.read_block(sector_id, &mut sector_buf)
                            .map_err(|_| VfsError::NotSupported)?;

                        let chunk_size = core::cmp::min(
                            buf.len() - buf_offset,
                            (sector_size - sector_offset as u64) as usize
                        );
                        buf[buf_offset..buf_offset + chunk_size]
                            .copy_from_slice(&sector_buf[sector_offset..sector_offset + chunk_size]);

                        buf_offset += chunk_size;
                        read_offset += chunk_size as u64;
                    }
                    return Ok(());
                }
            }
        }
        Err(VfsError::NotFound)
    }
}

/// Representation of an Ext2 Inode.
#[derive(Clone, Debug)]
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
            block[i] = u32::from_le_bytes([data[offset], data[offset+1], data[offset+2], data[offset+3]]);
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

    pub fn is_dir(&self) -> bool {
        (self.mode & 0xF000) == 0x4000
    }

    pub fn is_file(&self) -> bool {
        (self.mode & 0xF000) == 0x8000
    }
}

/// Representation of an active Ext2 filesystem volume instance.
pub struct Ext2Volume {
    pub reader: BlockDeviceReader,
    pub sb: Ext2Superblock,
}

impl Ext2Volume {
    pub fn new(device_name: &'static str) -> Result<Self, VfsError> {
        let reader = BlockDeviceReader::new(device_name);
        
        let mut sb_buf = alloc::vec![0u8; 1024];
        reader.read_bytes(1024, &mut sb_buf)?;

        let sb = Ext2Superblock::parse(&sb_buf).ok_or(VfsError::InvalidInput)?;

        Ok(Self { reader, sb })
    }

    /// Read an Ext2 inode by its 1-based index number.
    pub fn read_inode(&self, ino: u32) -> Result<Ext2Inode, VfsError> {
        if ino == 0 || ino > self.sb.inodes_count {
            return Err(VfsError::NotFound);
        }

        let group = (ino - 1) / self.sb.inodes_per_group;
        let index = (ino - 1) % self.sb.inodes_per_group;

        let bg_desc_table_block = if self.sb.block_size == 1024 { 2 } else { 1 };
        let bg_desc_offset = (bg_desc_table_block * self.sb.block_size) as u64 + (group as u64 * 32);

        let mut bg_buf = alloc::vec![0u8; 32];
        self.reader.read_bytes(bg_desc_offset, &mut bg_buf)?;

        let bg = Ext2BlockGroupDescriptor::parse(&bg_buf).ok_or(VfsError::InvalidInput)?;

        let inode_offset = (bg.inode_table as u64 * self.sb.block_size as u64) + (index as u64 * self.sb.inode_size as u64);

        let mut inode_buf = alloc::vec![0u8; self.sb.inode_size as usize];
        self.reader.read_bytes(inode_offset, &mut inode_buf)?;

        Ext2Inode::parse(&inode_buf).ok_or(VfsError::InvalidInput)
    }

    /// Resolve the physical block number for the virtual block offset within the inode.
    pub fn get_inode_block(&self, inode: &Ext2Inode, block_offset: u32) -> Result<u32, VfsError> {
        let block_size = self.sb.block_size;
        let ptrs_per_block = block_size / 4;

        if block_offset < 12 {
            return Ok(inode.block[block_offset as usize]);
        }

        let mut indirect_offset = block_offset - 12;

        if indirect_offset < ptrs_per_block {
            let singly_indirect_block = inode.block[12];
            if singly_indirect_block == 0 {
                return Ok(0);
            }
            let offset = singly_indirect_block as u64 * block_size as u64 + indirect_offset as u64 * 4;
            let mut ptr_buf = [0u8; 4];
            self.reader.read_bytes(offset, &mut ptr_buf)?;
            return Ok(u32::from_le_bytes(ptr_buf));
        }

        indirect_offset -= ptrs_per_block;

        if indirect_offset < ptrs_per_block * ptrs_per_block {
            let doubly_indirect_block = inode.block[13];
            if doubly_indirect_block == 0 {
                return Ok(0);
            }
            
            let singly_index = indirect_offset / ptrs_per_block;
            let direct_index = indirect_offset % ptrs_per_block;

            let singly_offset = doubly_indirect_block as u64 * block_size as u64 + singly_index as u64 * 4;
            let mut singly_ptr_buf = [0u8; 4];
            self.reader.read_bytes(singly_offset, &mut singly_ptr_buf)?;
            let singly_indirect_block = u32::from_le_bytes(singly_ptr_buf);

            if singly_indirect_block == 0 {
                return Ok(0);
            }

            let direct_offset = singly_indirect_block as u64 * block_size as u64 + direct_index as u64 * 4;
            let mut direct_ptr_buf = [0u8; 4];
            self.reader.read_bytes(direct_offset, &mut direct_ptr_buf)?;
            return Ok(u32::from_le_bytes(direct_ptr_buf));
        }

        Err(VfsError::NotSupported)
    }

    /// Read data from an inode at specified offset into the buffer.
    pub fn read_inode_data(&self, inode: &Ext2Inode, offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
        let size = inode.size as usize;
        if offset >= size {
            return Ok(0);
        }

        let read_len = core::cmp::min(buf.len(), size - offset);
        let block_size = self.sb.block_size as usize;

        let mut bytes_read = 0;

        while bytes_read < read_len {
            let current_offset = offset + bytes_read;
            let block_offset = (current_offset / block_size) as u32;
            let block_internal_offset = current_offset % block_size;

            let physical_block = self.get_inode_block(inode, block_offset)?;
            
            let chunk = core::cmp::min(read_len - bytes_read, block_size - block_internal_offset);

            if physical_block == 0 {
                // Sparse block, zero it
                for i in 0..chunk {
                    buf[bytes_read + i] = 0;
                }
            } else {
                let phys_offset = physical_block as u64 * self.sb.block_size as u64 + block_internal_offset as u64;
                self.reader.read_bytes(phys_offset, &mut buf[bytes_read..bytes_read + chunk])?;
            }

            bytes_read += chunk;
        }

        Ok(bytes_read)
    }
}

/// Ext2 VFS Inode Operations dispatch table.
pub struct Ext2InodeOps {
    pub volume: Arc<Ext2Volume>,
    pub ino: u32,
}

impl InodeOps for Ext2InodeOps {
    fn lookup(&self, name: &str) -> Result<Arc<Inode>, VfsError> {
        let dir_inode = self.volume.read_inode(self.ino)?;
        if !dir_inode.is_dir() {
            return Err(VfsError::NotDirectory);
        }

        let child_ino = super::dir::ext2_lookup(&self.volume, &dir_inode, name)?;
        let child_inode = self.volume.read_inode(child_ino)?;

        let inode_type = if child_inode.is_dir() {
            InodeType::Directory
        } else if child_inode.is_file() {
            InodeType::File
        } else {
            InodeType::File
        };

        Ok(Arc::new(Inode {
            ino: child_ino as u64,
            inode_type,
            ops: Arc::new(Ext2InodeOps {
                volume: self.volume.clone(),
                ino: child_ino,
            }),
        }))
    }

    fn readdir(&self) -> Result<Vec<String>, VfsError> {
        let dir_inode = self.volume.read_inode(self.ino)?;
        if !dir_inode.is_dir() {
            return Err(VfsError::NotDirectory);
        }
        super::dir::ext2_readdir(&self.volume, &dir_inode)
    }

    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        let inode = self.volume.read_inode(self.ino)?;
        if inode.is_dir() {
            return Err(VfsError::NotFile);
        }
        Ok(Arc::new(Ext2FileOps {
            volume: self.volume.clone(),
            ino: self.ino,
        }))
    }
}
