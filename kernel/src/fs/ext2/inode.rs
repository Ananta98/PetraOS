use super::bitmap::Ext2Bitmap;
use super::dir::{ext2_add_entry, ext2_is_dir_empty, ext2_lookup, ext2_readdir, ext2_remove_entry};
use super::file::Ext2FileOps;
pub use super::ondisk::Ext2Inode;
pub use super::reader::BlockDeviceReader;
use super::superblock::{Ext2BlockGroupDescriptor, Ext2Superblock};
use crate::fs::vfs::types::{
    FileOps, Inode, InodeOps, InodeType, MODE_PERM_BITS, MODE_TYPE_BITS, Stat, VfsError,
};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

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
        if sb.magic != 0xEF53 || (sb.rev_level != 0 && sb.rev_level != 1) || sb.log_block_size > 2 {
            return Err(VfsError::InvalidInput);
        }

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

    /// Resolve or allocate physical block for virtual block index (direct, singly, and doubly indirect).
    pub fn get_or_alloc_inode_block(
        &self,
        inode: &mut Ext2Inode,
        block_offset: u32,
    ) -> Result<u32, VfsError> {
        let block_size = self.sb.block_size;
        let n = block_size / 4;

        if block_offset < 12 {
            if inode.block[block_offset as usize] == 0 {
                let new_block = Ext2Bitmap::alloc_block(self)?;
                inode.block[block_offset as usize] = new_block;
                inode.blocks += block_size / 512;
            }
            return Ok(inode.block[block_offset as usize]);
        }

        let mut offset = block_offset - 12;

        // 1. Singly Indirect Allocation
        if offset < n {
            if inode.block[12] == 0 {
                let new_singly = Ext2Bitmap::alloc_block(self)?;
                inode.block[12] = new_singly;
                inode.blocks += block_size / 512;
                let zero_buf = alloc::vec![0u8; block_size as usize];
                self.reader
                    .write_bytes(new_singly as u64 * block_size as u64, &zero_buf)?;
            }
            let singly_block = inode.block[12];
            let phys_ptr = singly_block as u64 * block_size as u64 + offset as u64 * 4;
            let mut ptr_buf = [0u8; 4];
            self.reader.read_bytes(phys_ptr, &mut ptr_buf)?;
            let mut data_block = u32::from_le_bytes(ptr_buf);
            if data_block == 0 {
                data_block = Ext2Bitmap::alloc_block(self)?;
                self.reader
                    .write_bytes(phys_ptr, &data_block.to_le_bytes())?;
                inode.blocks += block_size / 512;
            }
            return Ok(data_block);
        }

        offset -= n;

        // 2. Doubly Indirect Allocation
        if offset < n * n {
            if inode.block[13] == 0 {
                let new_doubly = Ext2Bitmap::alloc_block(self)?;
                inode.block[13] = new_doubly;
                inode.blocks += block_size / 512;
                let zero_buf = alloc::vec![0u8; block_size as usize];
                self.reader
                    .write_bytes(new_doubly as u64 * block_size as u64, &zero_buf)?;
            }
            let doubly_block = inode.block[13];
            let i1 = offset / n;
            let i2 = offset % n;

            let p1_phys = doubly_block as u64 * block_size as u64 + i1 as u64 * 4;
            let mut ptr1_buf = [0u8; 4];
            self.reader.read_bytes(p1_phys, &mut ptr1_buf)?;
            let mut singly_block = u32::from_le_bytes(ptr1_buf);

            if singly_block == 0 {
                singly_block = Ext2Bitmap::alloc_block(self)?;
                self.reader
                    .write_bytes(p1_phys, &singly_block.to_le_bytes())?;
                inode.blocks += block_size / 512;
                let zero_buf = alloc::vec![0u8; block_size as usize];
                self.reader
                    .write_bytes(singly_block as u64 * block_size as u64, &zero_buf)?;
            }

            let p2_phys = singly_block as u64 * block_size as u64 + i2 as u64 * 4;
            let mut ptr2_buf = [0u8; 4];
            self.reader.read_bytes(p2_phys, &mut ptr2_buf)?;
            let mut data_block = u32::from_le_bytes(ptr2_buf);

            if data_block == 0 {
                data_block = Ext2Bitmap::alloc_block(self)?;
                self.reader
                    .write_bytes(p2_phys, &data_block.to_le_bytes())?;
                inode.blocks += block_size / 512;
            }

            return Ok(data_block);
        }

        Err(VfsError::NotSupported)
    }

    /// Resolve physical block number for virtual block offset (direct, singly, doubly, triply indirect).
    pub fn get_inode_block(&self, inode: &Ext2Inode, block_offset: u32) -> Result<u32, VfsError> {
        let block_size = self.sb.block_size;
        let n = block_size / 4;

        if block_offset < 12 {
            return Ok(inode.block[block_offset as usize]);
        }

        let mut offset = block_offset - 12;

        // 1. Singly Indirect (block[12])
        if offset < n {
            let singly_block = inode.block[12];
            if singly_block == 0 {
                return Ok(0);
            }
            let phys = singly_block as u64 * block_size as u64 + offset as u64 * 4;
            let mut ptr_buf = [0u8; 4];
            self.reader.read_bytes(phys, &mut ptr_buf)?;
            return Ok(u32::from_le_bytes(ptr_buf));
        }

        offset -= n;

        // 2. Doubly Indirect (block[13])
        if offset < n * n {
            let doubly_block = inode.block[13];
            if doubly_block == 0 {
                return Ok(0);
            }
            let i1 = offset / n;
            let i2 = offset % n;

            let p1_phys = doubly_block as u64 * block_size as u64 + i1 as u64 * 4;
            let mut ptr1_buf = [0u8; 4];
            self.reader.read_bytes(p1_phys, &mut ptr1_buf)?;
            let singly_block = u32::from_le_bytes(ptr1_buf);
            if singly_block == 0 {
                return Ok(0);
            }

            let p2_phys = singly_block as u64 * block_size as u64 + i2 as u64 * 4;
            let mut ptr2_buf = [0u8; 4];
            self.reader.read_bytes(p2_phys, &mut ptr2_buf)?;
            return Ok(u32::from_le_bytes(ptr2_buf));
        }

        offset -= n * n;

        // 3. Triply Indirect (block[14])
        if offset < n * n * n {
            let triply_block = inode.block[14];
            if triply_block == 0 {
                return Ok(0);
            }
            let i1 = offset / (n * n);
            let i2 = (offset / n) % n;
            let i3 = offset % n;

            let p1_phys = triply_block as u64 * block_size as u64 + i1 as u64 * 4;
            let mut ptr1_buf = [0u8; 4];
            self.reader.read_bytes(p1_phys, &mut ptr1_buf)?;
            let doubly_block = u32::from_le_bytes(ptr1_buf);
            if doubly_block == 0 {
                return Ok(0);
            }

            let p2_phys = doubly_block as u64 * block_size as u64 + i2 as u64 * 4;
            let mut ptr2_buf = [0u8; 4];
            self.reader.read_bytes(p2_phys, &mut ptr2_buf)?;
            let singly_block = u32::from_le_bytes(ptr2_buf);
            if singly_block == 0 {
                return Ok(0);
            }

            let p3_phys = singly_block as u64 * block_size as u64 + i3 as u64 * 4;
            let mut ptr3_buf = [0u8; 4];
            self.reader.read_bytes(p3_phys, &mut ptr3_buf)?;
            return Ok(u32::from_le_bytes(ptr3_buf));
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
        } else if child_inode.is_symlink() {
            InodeType::Symlink
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
            uid: inode.uid as u32,
            gid: inode.gid as u32,
            size: inode.size as u64,
            atime: inode.atime as u64,
            mtime: inode.mtime as u64,
            ctime: inode.ctime as u64,
            blksize: self.volume.sb.block_size as u64,
            blocks: inode.blocks as u64,
        })
    }

    /// Change permission bits of the on-disk inode, preserving type bits.
    fn chmod(&self, mode: u32) -> Result<(), VfsError> {
        let mut inode = self.volume.read_inode(self.ino)?;
        inode.mode = ((inode.mode as u32 & MODE_TYPE_BITS) | (mode & MODE_PERM_BITS)) as u16;
        inode.ctime = crate::drivers::time::cmos_rtc::get_wall_time().0 as u32;
        self.volume.write_inode(self.ino, &inode)
    }

    /// Change ownership of the on-disk inode (truncated to the 16-bit ext2 fields).
    fn chown(&self, uid: u32, gid: u32) -> Result<(), VfsError> {
        let mut inode = self.volume.read_inode(self.ino)?;
        inode.uid = uid as u16;
        inode.gid = gid as u16;
        inode.ctime = crate::drivers::time::cmos_rtc::get_wall_time().0 as u32;
        self.volume.write_inode(self.ino, &inode)
    }

    /// Update access and modification timestamps of the on-disk inode.
    fn utimens(&self, atime: u64, mtime: u64) -> Result<(), VfsError> {
        let mut inode = self.volume.read_inode(self.ino)?;
        inode.atime = atime as u32;
        inode.mtime = mtime as u32;
        inode.ctime = crate::drivers::time::cmos_rtc::get_wall_time().0 as u32;
        self.volume.write_inode(self.ino, &inode)
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
