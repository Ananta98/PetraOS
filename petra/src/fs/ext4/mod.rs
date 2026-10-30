// SPDX-License-Identifier: GPL-2.0

//! Ext4 Filesystem Driver Module.
//!
//! Provides full read-write Ext4 (and Ext2/Ext3 compatible) filesystem support
//! over [`BlockDevice`], including extents, 64-bit block addressing, directory
//! operations, truncation, and journal replay at mount time.

pub mod alloc;
pub mod blockgroup;
pub mod dir;
pub mod extents;
pub mod fs;
pub mod htree;
pub mod indirect;
pub mod inode;
pub mod journal;
pub mod superblock;

use ::alloc::sync::Arc;

use crate::{
    drivers::block::BlockDevice,
    fs::{
        registry::{FileSystemType, register_filesystem_type},
        vfs::FileSystem,
    },
    uapi::errno::Result,
};

/// Filesystem type descriptor for Ext4.
pub(crate) struct Ext4FsType;

impl FileSystemType for Ext4FsType {
    fn name(&self) -> &'static str {
        "ext4"
    }

    fn mount(&self, dev: Arc<dyn BlockDevice>) -> Result<Arc<dyn FileSystem>> {
        let fs = fs::Ext4Fs::new(dev)?;
        Ok(fs as _)
    }
}

fn init() -> Result<()> {
    register_filesystem_type(Arc::new(Ext4FsType));
    Ok(())
}

crate::module!(
    "Ext4 Filesystem Driver",
    "Ananta98",
    crate::modules::InitcallLevel::Fs,
    init
);
