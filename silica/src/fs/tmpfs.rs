// SPDX-License-Identifier: GPL-2.0

//! In-memory temporary filesystem (tmpfs / ramfs).

use alloc::{boxed::Box, sync::Arc};

use crate::api::errno::Result;
use crate::fs::initramfs::InitramfsDirNodeOps;
use crate::fs::vfs::{FileSystem, INode, Mode, StatFs};

/// In-memory RAM-backed temporary filesystem.
pub struct TmpFs {
    root: Arc<INode>,
}

impl TmpFs {
    /// Creates a new TmpFs instance with an empty root directory.
    pub fn new() -> Self {
        let dir_ops = Box::new(InitramfsDirNodeOps::new());
        let root = Arc::new(INode::new(
            dir_ops,
            Mode::DIR
                | Mode::RUSR
                | Mode::WUSR
                | Mode::XUSR
                | Mode::RGRP
                | Mode::WGRP
                | Mode::XGRP
                | Mode::ROTH
                | Mode::WOTH
                | Mode::XOTH,
            1,
        ));
        Self { root }
    }
}

impl Default for TmpFs {
    fn default() -> Self {
        Self::new()
    }
}

impl FileSystem for TmpFs {
    fn name(&self) -> &'static str {
        "tmpfs"
    }

    fn root(&self) -> Result<Arc<INode>> {
        Ok(Arc::clone(&self.root))
    }

    fn statfs(&self) -> Result<StatFs> {
        Ok(StatFs {
            bsize: 4096,
            blocks: 1024 * 1024,
            bfree: 1024 * 1024,
            bavail: 1024 * 1024,
            files: 1024 * 1024,
            ffree: 1024 * 1024,
            namelen: 255,
        })
    }
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use crate::fs::vfs::OpenFlags;
    use ostd::prelude::ktest;

    #[ktest]
    fn tmpfs_create_write_read_and_stat() {
        let tmpfs = TmpFs::new();
        let root = tmpfs.root().expect("tmpfs root");

        // Create a regular file in tmpfs
        let file_node = root
            .node_ops
            .create("alpine_test.txt", Mode::FILE | Mode::RUSR | Mode::WUSR)
            .expect("create file");

        let file_ops = file_node
            .node_ops
            .open(OpenFlags::READ | OpenFlags::WRITE)
            .expect("open file");

        let test_data = b"Alpine Linux tmpfs verification payload";
        let written = file_ops.write(0, test_data).expect("write data");
        assert_eq!(written, test_data.len());

        let mut read_buf = [0u8; 64];
        let bytes_read = file_ops.read(0, &mut read_buf).expect("read data");
        assert_eq!(bytes_read, test_data.len());
        assert_eq!(&read_buf[..bytes_read], test_data);

        // Verify lookup in root
        let lookup_node = root
            .node_ops
            .lookup("alpine_test.txt")
            .expect("lookup file");
        let attr = lookup_node.node_ops.getattr().expect("getattr");
        assert_eq!(attr.size, test_data.len());
    }
}
