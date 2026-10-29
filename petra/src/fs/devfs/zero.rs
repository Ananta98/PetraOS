use alloc::boxed::Box;

use crate::fs::vfs::{FileOps, INodeAttr, Mode, NodeOps, OpenFlags};
use crate::uapi::errno::Result;

/// /dev/zero - Discards all writes, reads return infinite zero-bytes.
pub struct ZeroFile;

impl FileOps for ZeroFile {
    fn read(&self, _offset: u64, buf: &mut [u8]) -> Result<usize> {
        buf.fill(0);
        Ok(buf.len())
    }

    fn write(&self, _offset: u64, buf: &[u8]) -> Result<usize> {
        Ok(buf.len())
    }

    fn mmap(&self, _offset: u64, _len: usize) -> Result<()> {
        Ok(())
    }
}

pub struct ZeroNodeOps;

impl NodeOps for ZeroNodeOps {
    fn open(&self, _flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        Ok(Box::new(ZeroFile))
    }

    fn getattr(&self) -> Result<INodeAttr> {
        Ok(INodeAttr {
            size: 0,
            mode: Mode::CHAR
                | Mode::RUSR
                | Mode::WUSR
                | Mode::RGRP
                | Mode::WGRP
                | Mode::ROTH
                | Mode::WOTH,
            nlink: 1,
            uid: 0,
            gid: 0,
            rdev: ((1 as u64) << 8) | 5,
        })
    }
}
