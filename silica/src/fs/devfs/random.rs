// SPDX-License-Identifier: GPL-2.0

//! Virtual Random Number Devices (`/dev/random` and `/dev/urandom`).
//!
//! Provides character devices backed by the ChaCha20 kernel CSPRNG and
//! multi-source entropy harvester. Reads output cryptographically secure
//! random bytes; writes feed external entropy back into the pool.

use crate::{
    uapi::errno::Result,
    fs::vfs::{FileOps, INodeAttr, Mode, NodeOps, OpenFlags},
};
use alloc::boxed::Box;

/// `/dev/random` file operations.
pub struct RandomFile;

impl FileOps for RandomFile {
    fn read(&self, _offset: u64, buf: &mut [u8]) -> Result<usize> {
        crate::crypto::get_random_bytes(buf);
        Ok(buf.len())
    }

    fn write(&self, _offset: u64, buf: &[u8]) -> Result<usize> {
        crate::crypto::add_entropy(buf);
        Ok(buf.len())
    }
}

/// Inode operations for `/dev/random`.
pub struct RandomNodeOps;

impl NodeOps for RandomNodeOps {
    fn open(&self, _flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        Ok(Box::new(RandomFile))
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
            rdev: 0,
        })
    }
}

impl RandomFile {
    /// Helper to create an open File description for `/dev/random`.
    pub fn new_file() -> alloc::sync::Arc<crate::fs::vfs::File> {
        let inode = alloc::sync::Arc::new(crate::fs::vfs::INode::new(
            Box::new(RandomNodeOps),
            Mode::CHAR
                | Mode::RUSR
                | Mode::WUSR
                | Mode::RGRP
                | Mode::WGRP
                | Mode::ROTH
                | Mode::WOTH,
            3,
        ));
        alloc::sync::Arc::new(crate::fs::vfs::File::new(
            Box::new(RandomFile),
            inode,
            OpenFlags::READ | OpenFlags::WRITE,
            false,
        ))
    }
}

/// `/dev/urandom` file operations.
pub struct UrandomFile;

impl FileOps for UrandomFile {
    fn read(&self, _offset: u64, buf: &mut [u8]) -> Result<usize> {
        crate::crypto::get_random_bytes(buf);
        Ok(buf.len())
    }

    fn write(&self, _offset: u64, buf: &[u8]) -> Result<usize> {
        crate::crypto::add_entropy(buf);
        Ok(buf.len())
    }
}

/// Inode operations for `/dev/urandom`.
pub struct UrandomNodeOps;

impl NodeOps for UrandomNodeOps {
    fn open(&self, _flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        Ok(Box::new(UrandomFile))
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
            rdev: 0,
        })
    }
}

impl UrandomFile {
    /// Helper to create an open File description for `/dev/urandom`.
    pub fn new_file() -> alloc::sync::Arc<crate::fs::vfs::File> {
        let inode = alloc::sync::Arc::new(crate::fs::vfs::INode::new(
            Box::new(UrandomNodeOps),
            Mode::CHAR
                | Mode::RUSR
                | Mode::WUSR
                | Mode::RGRP
                | Mode::WGRP
                | Mode::ROTH
                | Mode::WOTH,
            4,
        ));
        alloc::sync::Arc::new(crate::fs::vfs::File::new(
            Box::new(UrandomFile),
            inode,
            OpenFlags::READ | OpenFlags::WRITE,
            false,
        ))
    }
}
