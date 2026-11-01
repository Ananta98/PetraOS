// SPDX-License-Identifier: GPL-2.0

//! Pseudo-Terminal Filesystem (`devpts`).
//!
//! Provides the `/dev/pts` pseudo-filesystem containing pseudo-terminal
//! slave devices (`0`, `1`, ...) and the master multiplexer (`ptmx`).

use alloc::boxed::Box;
use alloc::sync::Arc;

use crate::{
    drivers::char::tty::pty::PtsDirNodeOps,
    fs::vfs::{FileSystem, INode, Mode, StatFs},
    uapi::errno::Result,
};

/// The DevPts filesystem representation.
pub struct DevPtsFs {
    root: Arc<INode>,
}

impl DevPtsFs {
    pub fn new() -> Self {
        let dir_ops = Box::new(PtsDirNodeOps);
        let root = Arc::new(INode::new(
            dir_ops,
            Mode::DIR
                | Mode::RUSR
                | Mode::WUSR
                | Mode::XUSR
                | Mode::RGRP
                | Mode::XGRP
                | Mode::ROTH
                | Mode::XOTH,
            0,
        ));
        Self { root }
    }
}

impl Default for DevPtsFs {
    fn default() -> Self {
        Self::new()
    }
}

impl FileSystem for DevPtsFs {
    fn name(&self) -> &'static str {
        "devpts"
    }

    fn root(&self) -> Result<Arc<INode>> {
        Ok(Arc::clone(&self.root))
    }

    fn statfs(&self) -> Result<StatFs> {
        Ok(StatFs {
            f_type: 0x0000_1cd1, // DEVPTS_SUPER_MAGIC
            bsize: 4096,
            blocks: 0,
            bfree: 0,
            bavail: 0,
            files: 0,
            ffree: 0,
            namelen: 255,
        })
    }
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;

    #[ktest]
    fn devpts_filesystem_root_and_ptmx() {
        let devpts = DevPtsFs::new();
        assert_eq!(devpts.name(), "devpts");

        let root = devpts.root().expect("devpts root inode");
        assert!(root.is_dir());

        let ptmx = root.lookup("ptmx").expect("devpts /ptmx lookup");
        assert!(ptmx.is_chr());

        let statfs = devpts.statfs().expect("devpts statfs");
        assert_eq!(statfs.f_type, 0x0000_1cd1);
    }
}
