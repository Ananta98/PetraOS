use crate::fs::vfs::{FileOps, FileType, InodeOps, Metadata, Result as VfsResult, SeekFrom};
use alloc::boxed::Box;
use alloc::sync::Arc;
use ostd::Error;
use ostd::sync::SpinLock;

pub struct TimerFdNode {
    pub timer_ticks: SpinLock<u64>,
}

impl InodeOps for TimerFdNode {
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
        Ok(Box::new(TimerFdOps {
            node: Arc::new(TimerFdNode {
                timer_ticks: SpinLock::new(1),
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

pub struct TimerFdOps {
    pub node: Arc<TimerFdNode>,
}

impl FileOps for TimerFdOps {
    fn read(&mut self, buf: &mut [u8], _: &mut usize) -> VfsResult<usize> {
        if buf.len() < 8 {
            return Err(Error::InvalidArgs);
        }
        let ticks = *self.node.timer_ticks.lock();
        buf[..8].copy_from_slice(&ticks.to_ne_bytes());
        Ok(8)
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
