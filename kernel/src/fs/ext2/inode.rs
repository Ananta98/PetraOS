use super::dir::{ext2_add_entry, ext2_is_dir_empty, ext2_lookup, ext2_readdir, ext2_remove_entry};
use super::bitmap::Ext2Bitmap;
use super::file::Ext2FileOps;
use super::superblock::{Ext2BlockGroupDescriptor, Ext2Superblock};
use crate::device::DEVICE_MANAGER;
use crate::fs::vfs::types::{FileOps, Inode, InodeOps, InodeType, Stat, VfsError};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

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
            if dev_lock.as_ref().name() == self.device_name {
                if let Some(block_dev) = dev_lock.as_mut().as_block_device_mut() {
                    let sector_size = block_dev.block_size() as u64;
                    let mut read_offset = offset;
                    let mut buf_offset = 0;
                    let mut sector_buf = alloc::vec![0u8; sector_size as usize];

                    while buf_offset < buf.len() {
                        let sector_id = read_offset / sector_size;
                        let sector_offset = (read_offset % sector_size) as usize;

                        block_dev
                            .read_block(sector_id, &mut sector_buf)
                            .map_err(|_| VfsError::NotSupported)?;

                        let chunk_size = core::cmp::min(
                            buf.len() - buf_offset,
                            (sector_size - sector_offset as u64) as usize,
                        );
                        buf[buf_offset..buf_offset + chunk_size].copy_from_slice(
                            &sector_buf[sector_offset..sector_offset + chunk_size],
                        );

                        buf_offset += chunk_size;
                        read_offset += chunk_size as u64;
                    }
                    return Ok(());
                }
            }
        }
        Err(VfsError::NotFound)
    }

    /// Write `buf.len()` bytes starting at `offset` (in bytes).
    pub fn write_bytes(&self, offset: u64, buf: &[u8]) -> Result<(), VfsError> {
        let dm = DEVICE_MANAGER.lock();
        for dev_arc in dm.get_devices() {
            let mut dev_lock = dev_arc.lock();
            if dev_lock.as_ref().name() == self.device_name {
                if let Some(block_dev) = dev_lock.as_mut().as_block_device_mut() {
                    let sector_size = block_dev.block_size() as u64;
                    let mut write_offset = offset;
                    let mut buf_offset = 0;
                    let mut sector_buf = alloc::vec![0u8; sector_size as usize];

                    while buf_offset < buf.len() {
                        let sector_id = write_offset / sector_size;
                        let sector_offset = (write_offset % sector_size) as usize;
                        let chunk_size = core::cmp::min(
                            buf.len() - buf_offset,
                            (sector_size - sector_offset as u64) as usize,
                        );

                        if sector_offset != 0 || chunk_size < sector_size as usize {
                            block_dev
                                .read_block(sector_id, &mut sector_buf)
                                .map_err(|_| VfsError::NotSupported)?;
                        }

                        sector_buf[sector_offset..sector_offset + chunk_size]
                            .copy_from_slice(&buf[buf_offset..buf_offset + chunk_size]);

                        block_dev
                            .write_block(sector_id, &sector_buf)
                            .map_err(|_| VfsError::NotSupported)?;

                        buf_offset += chunk_size;
                        write_offset += chunk_size as u64;
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
        log::info!(
            "[AHCI Ext2] Verified Ext2 Superblock magic: 0x{:04X}, rev_level: {}, log_block_size: {} (block_size: {} bytes)",
            sb.magic,
            sb.rev_level,
            sb.log_block_size,
            sb.block_size
        );
        assert_eq!(sb.magic, 0xEF53, "Ext2 magic number mismatch");
        assert_eq!(sb.rev_level, 0, "Ext2 revision level mismatch");
        assert_eq!(sb.log_block_size, 0, "Ext2 log_block_size mismatch");

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
        let bg_desc_offset =
            (bg_desc_table_block * self.sb.block_size) as u64 + (group as u64 * 32);

        let mut bg_buf = alloc::vec![0u8; 32];
        self.reader.read_bytes(bg_desc_offset, &mut bg_buf)?;

        let bg = Ext2BlockGroupDescriptor::parse(&bg_buf).ok_or(VfsError::InvalidInput)?;

        let inode_offset = (bg.inode_table as u64 * self.sb.block_size as u64)
            + (index as u64 * self.sb.inode_size as u64);

        let mut inode_buf = alloc::vec![0u8; self.sb.inode_size as usize];
        self.reader.read_bytes(inode_offset, &mut inode_buf)?;

        Ext2Inode::parse(&inode_buf).ok_or(VfsError::InvalidInput)
    }

    /// Write an Ext2 inode back to disk.
    pub fn write_inode(&self, ino: u32, inode: &Ext2Inode) -> Result<(), VfsError> {
        if ino == 0 || ino > self.sb.inodes_count {
            return Err(VfsError::NotFound);
        }

        let group = (ino - 1) / self.sb.inodes_per_group;
        let index = (ino - 1) % self.sb.inodes_per_group;

        let bg_desc_table_block = if self.sb.block_size == 1024 { 2 } else { 1 };
        let bg_desc_offset =
            (bg_desc_table_block * self.sb.block_size) as u64 + (group as u64 * 32);

        let mut bg_buf = alloc::vec![0u8; 32];
        self.reader.read_bytes(bg_desc_offset, &mut bg_buf)?;
        let bg = Ext2BlockGroupDescriptor::parse(&bg_buf).ok_or(VfsError::InvalidInput)?;

        let inode_offset = (bg.inode_table as u64 * self.sb.block_size as u64)
            + (index as u64 * self.sb.inode_size as u64);

        let bytes = inode.serialize();
        self.reader.write_bytes(inode_offset, &bytes)
    }

    /// Resolve or allocate physical block for virtual block index.
    pub fn get_or_alloc_inode_block(
        &self,
        inode: &mut Ext2Inode,
        block_offset: u32,
    ) -> Result<u32, VfsError> {
        if block_offset < 12 {
            if inode.block[block_offset as usize] == 0 {
                let new_block = Ext2Bitmap::alloc_block(self)?;
                inode.block[block_offset as usize] = new_block;
                inode.blocks += self.sb.block_size / 512;
            }
            return Ok(inode.block[block_offset as usize]);
        }
        Err(VfsError::NotSupported)
    }

    /// Resolve physical block number for virtual block offset.
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
            let offset =
                singly_indirect_block as u64 * block_size as u64 + indirect_offset as u64 * 4;
            let mut ptr_buf = [0u8; 4];
            self.reader.read_bytes(offset, &mut ptr_buf)?;
            return Ok(u32::from_le_bytes(ptr_buf));
        }

        Err(VfsError::NotSupported)
    }

    /// Read data from an inode.
    pub fn read_inode_data(
        &self,
        inode: &Ext2Inode,
        offset: usize,
        buf: &mut [u8],
    ) -> Result<usize, VfsError> {
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
                for i in 0..chunk {
                    buf[bytes_read + i] = 0;
                }
            } else {
                let phys_offset = physical_block as u64 * self.sb.block_size as u64
                    + block_internal_offset as u64;
                self.reader
                    .read_bytes(phys_offset, &mut buf[bytes_read..bytes_read + chunk])?;
            }

            bytes_read += chunk;
        }

        Ok(bytes_read)
    }

    /// Write data to an inode.
    pub fn write_inode_data(
        &self,
        inode: &mut Ext2Inode,
        ino: u32,
        offset: usize,
        buf: &[u8],
    ) -> Result<usize, VfsError> {
        let block_size = self.sb.block_size as usize;
        let mut bytes_written = 0;

        while bytes_written < buf.len() {
            let current_offset = offset + bytes_written;
            let block_offset = (current_offset / block_size) as u32;
            let block_internal_offset = current_offset % block_size;

            let physical_block = self.get_or_alloc_inode_block(inode, block_offset)?;

            let chunk = core::cmp::min(
                buf.len() - bytes_written,
                block_size - block_internal_offset,
            );
            let phys_offset =
                physical_block as u64 * self.sb.block_size as u64 + block_internal_offset as u64;

            self.reader
                .write_bytes(phys_offset, &buf[bytes_written..bytes_written + chunk])?;

            bytes_written += chunk;
        }

        if offset + bytes_written > inode.size as usize {
            inode.size = (offset + bytes_written) as u32;
        }

        self.write_inode(ino, inode)?;
        Ok(bytes_written)
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

    fn create(&self, name: &str) -> Result<Arc<Inode>, VfsError> {
        let mut dir_inode = self.volume.read_inode(self.ino)?;
        if !dir_inode.is_dir() {
            return Err(VfsError::NotDirectory);
        }

        let child_ino = Ext2Bitmap::alloc_inode(&self.volume)?;
        let child_inode = Ext2Inode {
            mode: 0o100644,
            uid: 0,
            size: 0,
            atime: 0,
            ctime: 0,
            mtime: 0,
            dtime: 0,
            gid: 0,
            links_count: 1,
            blocks: 0,
            flags: 0,
            block: [0; 15],
        };

        self.volume.write_inode(child_ino, &child_inode)?;
        ext2_add_entry(&self.volume, &mut dir_inode, self.ino, name, child_ino, 1)?;

        Ok(Arc::new(Inode {
            ino: child_ino as u64,
            inode_type: InodeType::File,
            ops: Arc::new(Ext2InodeOps {
                volume: self.volume.clone(),
                ino: child_ino,
            }),
        }))
    }

    fn mkdir(&self, name: &str) -> Result<Arc<Inode>, VfsError> {
        let mut dir_inode = self.volume.read_inode(self.ino)?;
        if !dir_inode.is_dir() {
            return Err(VfsError::NotDirectory);
        }

        let child_ino = Ext2Bitmap::alloc_inode(&self.volume)?;
        let child_block = Ext2Bitmap::alloc_block(&self.volume)?;

        let mut child_inode = Ext2Inode {
            mode: 0o040755,
            uid: 0,
            size: self.volume.sb.block_size,
            atime: 0,
            ctime: 0,
            mtime: 0,
            dtime: 0,
            gid: 0,
            links_count: 2,
            blocks: self.volume.sb.block_size / 512,
            flags: 0,
            block: [0; 15],
        };
        child_inode.block[0] = child_block;

        self.volume.write_inode(child_ino, &child_inode)?;

        // Populate "." and ".." in child directory
        ext2_add_entry(&self.volume, &mut child_inode, child_ino, ".", child_ino, 2)?;
        ext2_add_entry(&self.volume, &mut child_inode, child_ino, "..", self.ino, 2)?;

        ext2_add_entry(&self.volume, &mut dir_inode, self.ino, name, child_ino, 2)?;

        dir_inode.links_count += 1;
        self.volume.write_inode(self.ino, &dir_inode)?;

        Ok(Arc::new(Inode {
            ino: child_ino as u64,
            inode_type: InodeType::Directory,
            ops: Arc::new(Ext2InodeOps {
                volume: self.volume.clone(),
                ino: child_ino,
            }),
        }))
    }

    fn unlink(&self, name: &str) -> Result<(), VfsError> {
        let mut dir_inode = self.volume.read_inode(self.ino)?;
        if !dir_inode.is_dir() {
            return Err(VfsError::NotDirectory);
        }

        let child_ino = ext2_remove_entry(&self.volume, &mut dir_inode, self.ino, name)?;
        let mut child_inode = self.volume.read_inode(child_ino)?;

        if child_inode.is_dir() {
            return Err(VfsError::IsDirectory);
        }

        child_inode.links_count = child_inode.links_count.saturating_sub(1);
        if child_inode.links_count == 0 {
            for b in child_inode.block.iter() {
                if *b != 0 {
                    Ext2Bitmap::free_block(&self.volume, *b)?;
                }
            }
            Ext2Bitmap::free_inode(&self.volume, child_ino)?;
        } else {
            self.volume.write_inode(child_ino, &child_inode)?;
        }

        Ok(())
    }

    fn rmdir(&self, name: &str) -> Result<(), VfsError> {
        let mut dir_inode = self.volume.read_inode(self.ino)?;
        if !dir_inode.is_dir() {
            return Err(VfsError::NotDirectory);
        }

        let child_ino = ext2_lookup(&self.volume, &dir_inode, name)?;
        let child_inode = self.volume.read_inode(child_ino)?;

        if !child_inode.is_dir() {
            return Err(VfsError::NotDirectory);
        }

        if !ext2_is_dir_empty(&self.volume, &child_inode)? {
            return Err(VfsError::NotEmpty);
        }

        ext2_remove_entry(&self.volume, &mut dir_inode, self.ino, name)?;

        for b in child_inode.block.iter() {
            if *b != 0 {
                Ext2Bitmap::free_block(&self.volume, *b)?;
            }
        }
        Ext2Bitmap::free_inode(&self.volume, child_ino)?;

        dir_inode.links_count = dir_inode.links_count.saturating_sub(1);
        self.volume.write_inode(self.ino, &dir_inode)?;

        Ok(())
    }

    fn readdir(&self) -> Result<Vec<String>, VfsError> {
        let dir_inode = self.volume.read_inode(self.ino)?;
        if !dir_inode.is_dir() {
            return Err(VfsError::NotDirectory);
        }
        ext2_readdir(&self.volume, &dir_inode)
    }

    fn stat(&self) -> Result<Stat, VfsError> {
        let inode = self.volume.read_inode(self.ino)?;
        Ok(Stat {
            ino: self.ino as u64,
            mode: inode.mode as u32,
            nlink: inode.links_count as u32,
            size: inode.size as u64,
            atime: inode.atime as u64,
            mtime: inode.mtime as u64,
            ctime: inode.ctime as u64,
            blksize: self.volume.sb.block_size as u64,
            blocks: inode.blocks as u64,
            ..Default::default()
        })
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
