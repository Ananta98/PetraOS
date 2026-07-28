use crate::fs::vfs::{FileOps, FileType, InodeOps, Metadata, Result as VfsResult, SeekFrom};
use alloc::boxed::Box;
use alloc::sync::Arc;
use ostd::Error;

pub struct SignalFdNode;

impl InodeOps for SignalFdNode {
    fn lookup(&self, _: &str) -> VfsResult<Arc<dyn InodeOps>> {
        Err(Error::InvalidArgs)
    }

    fn create(&self, _: &str, _: u32) -> VfsResult<Arc<dyn InodeOps>> {
        Err(Error::InvalidArgs)
    }

    fn mkdir(&self, _: &str, _: u32) -> VfsResult<Arc<dyn InodeOps>> {
        Err(Error::InvalidArgs)
    }

    fn symlink(&self, _: &str, _: &str) -> VfsResult<Arc<dyn InodeOps>> {
        Err(Error::InvalidArgs)
    }

    fn metadata(&self) -> VfsResult<Metadata> {
        Ok(Metadata {
            size: 128,
            file_type: FileType::Regular,
            mode: 0o600,
            uid: 0,
            gid: 0,
            inode_num: 1,
            nlink: 1,
        })
    }

    fn read_link(&self) -> VfsResult<alloc::string::String> {
        Err(Error::InvalidArgs)
    }

    fn open(&self, _: u32) -> VfsResult<Box<dyn FileOps>> {
        Ok(Box::new(SignalFdOps))
    }

    fn as_any(&self) -> &dyn core::any::Any {
        self
    }

    fn unlink(&self, _: &str) -> VfsResult<()> {
        Err(Error::InvalidArgs)
    }

    fn rename(&self, _: &str, _: &Arc<dyn InodeOps>, _: &str) -> VfsResult<()> {
        Err(Error::InvalidArgs)
    }
}

pub struct SignalFdOps;

impl FileOps for SignalFdOps {
    fn read(&mut self, _buf: &mut [u8], _: &mut usize) -> VfsResult<usize> {
        Ok(0)
    }

    fn write(&mut self, _: &[u8], _: &mut usize) -> VfsResult<usize> {
        Err(Error::InvalidArgs)
    }

    fn seek(&mut self, _: SeekFrom, _: &mut usize) -> VfsResult<usize> {
        Err(Error::InvalidArgs)
    }

    fn readdir(&mut self) -> VfsResult<alloc::vec::Vec<crate::fs::vfs::DirEntry>> {
        Err(Error::InvalidArgs)
    }
}
