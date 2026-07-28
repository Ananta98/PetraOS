use crate::fs::vfs::{FileOps, FileType, InodeOps, Metadata, Result as VfsResult, SeekFrom};
use alloc::boxed::Box;
use alloc::sync::Arc;
use ostd::Error;
use ostd::sync::SpinLock;

pub struct EventFdNode {
    pub counter: SpinLock<u64>,
}

impl InodeOps for EventFdNode {
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
            size: 8,
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
        Ok(Box::new(EventFdOps {
            node: Arc::new(EventFdNode {
                counter: SpinLock::new(0),
            }),
        }))
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

pub struct EventFdOps {
    pub node: Arc<EventFdNode>,
}

impl FileOps for EventFdOps {
    fn read(&mut self, buf: &mut [u8], _: &mut usize) -> VfsResult<usize> {
        if buf.len() < 8 {
            return Err(Error::InvalidArgs);
        }
        let mut val = self.node.counter.lock();
        let current = *val;
        *val = 0;
        buf[..8].copy_from_slice(&current.to_ne_bytes());
        Ok(8)
    }

    fn write(&mut self, buf: &[u8], _: &mut usize) -> VfsResult<usize> {
        if buf.len() < 8 {
            return Err(Error::InvalidArgs);
        }
        let mut add_buf = [0u8; 8];
        add_buf.copy_from_slice(&buf[..8]);
        let add_val = u64::from_ne_bytes(add_buf);
        let mut val = self.node.counter.lock();
        *val = val.saturating_add(add_val);
        Ok(8)
    }

    fn seek(&mut self, _: SeekFrom, _: &mut usize) -> VfsResult<usize> {
        Err(Error::InvalidArgs)
    }

    fn readdir(&mut self) -> VfsResult<alloc::vec::Vec<crate::fs::vfs::DirEntry>> {
        Err(Error::InvalidArgs)
    }
}
