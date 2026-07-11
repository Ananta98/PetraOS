use super::{Device, DeviceType, register_device};
use crate::fs::vfs::{DirEntry, FileOps, FileType, InodeOps, Metadata, SeekFrom};
use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
pub mod ahci;

pub fn init() {
    ahci::init();
}

pub trait BlockDevice: Send + Sync {
    fn block_size(&self) -> usize;
    fn num_blocks(&self) -> usize;
    fn read_blocks(&self, block_id: usize, buf: &mut [u8]) -> Result<(), ostd::Error>;
    fn write_blocks(&self, block_id: usize, buf: &[u8]) -> Result<(), ostd::Error>;
}

struct BlockDeviceWrapper {
    name: String,
    device: Arc<dyn BlockDevice>,
}

impl Device for BlockDeviceWrapper {
    fn name(&self) -> &str {
        &self.name
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Block
    }

    fn inode_ops(&self) -> Option<Arc<dyn InodeOps>> {
        Some(Arc::new(BlockDeviceInode::new(self.device.clone())))
    }
}

pub fn register_block_device(name: &str, device: Arc<dyn BlockDevice>) -> Result<(), ostd::Error> {
    let wrapper = Arc::new(BlockDeviceWrapper {
        name: String::from(name),
        device,
    });
    register_device(wrapper)
}

pub struct BlockDeviceInode {
    pub device: Arc<dyn BlockDevice>,
}

impl BlockDeviceInode {
    pub fn new(device: Arc<dyn BlockDevice>) -> Self {
        Self { device }
    }
}

impl InodeOps for BlockDeviceInode {
    fn lookup(&self, _name: &str) -> Result<Arc<dyn InodeOps>, ostd::Error> {
        Err(ostd::Error::InvalidArgs)
    }
    fn create(&self, _name: &str, _mode: u32) -> Result<Arc<dyn InodeOps>, ostd::Error> {
        Err(ostd::Error::InvalidArgs)
    }
    fn mkdir(&self, _name: &str, _mode: u32) -> Result<Arc<dyn InodeOps>, ostd::Error> {
        Err(ostd::Error::InvalidArgs)
    }
    fn symlink(&self, _name: &str, _target: &str) -> Result<Arc<dyn InodeOps>, ostd::Error> {
        Err(ostd::Error::InvalidArgs)
    }
    fn metadata(&self) -> Result<Metadata, ostd::Error> {
        let dev_size = self.device.block_size() * self.device.num_blocks();
        Ok(Metadata {
            size: dev_size,
            file_type: FileType::BlockDevice,
            mode: 0o660,
            inode_num: 0,
            nlink: 1,
        })
    }
    fn read_link(&self) -> Result<String, ostd::Error> {
        Err(ostd::Error::InvalidArgs)
    }
    fn open(&self, _flags: u32) -> Result<Box<dyn FileOps>, ostd::Error> {
        Ok(Box::new(BlockDeviceFile {
            device: self.device.clone(),
        }))
    }
    fn as_any(&self) -> &dyn core::any::Any {
        self
    }
    fn unlink(&self, _name: &str) -> Result<(), ostd::Error> {
        Err(ostd::Error::InvalidArgs)
    }
    fn rename(
        &self,
        _old_name: &str,
        _new_parent: &Arc<dyn InodeOps>,
        _new_name: &str,
    ) -> Result<(), ostd::Error> {
        Err(ostd::Error::InvalidArgs)
    }
}

pub struct BlockDeviceFile {
    device: Arc<dyn BlockDevice>,
}

impl FileOps for BlockDeviceFile {
    fn read(&mut self, buf: &mut [u8], offset: &mut usize) -> Result<usize, ostd::Error> {
        let block_size = self.device.block_size();
        let num_blocks = self.device.num_blocks();
        let dev_size = block_size * num_blocks;

        if *offset >= dev_size {
            return Ok(0);
        }

        let read_len = core::cmp::min(buf.len(), dev_size - *offset);
        let mut bytes_read = 0;
        let mut block_buf = alloc::vec![0u8; block_size];

        while bytes_read < read_len {
            let curr_offset = *offset + bytes_read;
            let block_id = curr_offset / block_size;
            let block_offset = curr_offset % block_size;
            let chunk_len = core::cmp::min(read_len - bytes_read, block_size - block_offset);

            self.device.read_blocks(block_id, &mut block_buf)?;
            buf[bytes_read..bytes_read + chunk_len]
                .copy_from_slice(&block_buf[block_offset..block_offset + chunk_len]);

            bytes_read += chunk_len;
        }

        *offset += bytes_read;
        Ok(bytes_read)
    }

    fn write(&mut self, buf: &[u8], offset: &mut usize) -> Result<usize, ostd::Error> {
        let block_size = self.device.block_size();
        let num_blocks = self.device.num_blocks();
        let dev_size = block_size * num_blocks;

        if *offset >= dev_size {
            return Err(ostd::Error::InvalidArgs);
        }

        let write_len = core::cmp::min(buf.len(), dev_size - *offset);
        let mut bytes_written = 0;
        let mut block_buf = alloc::vec![0u8; block_size];

        while bytes_written < write_len {
            let curr_offset = *offset + bytes_written;
            let block_id = curr_offset / block_size;
            let block_offset = curr_offset % block_size;
            let chunk_len = core::cmp::min(write_len - bytes_written, block_size - block_offset);

            if chunk_len < block_size {
                self.device.read_blocks(block_id, &mut block_buf)?;
            }
            block_buf[block_offset..block_offset + chunk_len]
                .copy_from_slice(&buf[bytes_written..bytes_written + chunk_len]);
            self.device.write_blocks(block_id, &block_buf)?;

            bytes_written += chunk_len;
        }

        *offset += bytes_written;
        Ok(bytes_written)
    }

    fn seek(&mut self, pos: SeekFrom, offset: &mut usize) -> Result<usize, ostd::Error> {
        let block_size = self.device.block_size();
        let num_blocks = self.device.num_blocks();
        let dev_size = block_size * num_blocks;

        let new_offset = match pos {
            SeekFrom::Start(val) => val as isize,
            SeekFrom::Current(val) => *offset as isize + val,
            SeekFrom::End(val) => dev_size as isize + val,
        };
        if new_offset < 0 || new_offset as usize > dev_size {
            return Err(ostd::Error::InvalidArgs);
        }
        *offset = new_offset as usize;
        Ok(*offset)
    }

    fn readdir(&mut self) -> Result<Vec<DirEntry>, ostd::Error> {
        Err(ostd::Error::InvalidArgs)
    }
}
