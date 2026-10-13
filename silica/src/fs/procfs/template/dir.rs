// SPDX-License-Identifier: GPL-2.0

//! ProcFS Directory Template and Operations.

use alloc::{boxed::Box, format, string::String, sync::Arc, vec::Vec};

use crate::{
    api::errno::Result,
    fs::vfs::{FileOps, INode, INodeAttr, Mode, NodeOps, OpenFlags},
};

/// Operations provided by a procfs directory.
pub trait ProcDirOps: Send + Sync + 'static {
    /// Looks up a child entry in this directory.
    fn lookup_child(&self, name: &str) -> Result<Arc<INode>>;

    /// Lists directory entries for `readdir`.
    fn list_entries(&self) -> Vec<String>;
}

/// A node operations adapter wrapping any `ProcDirOps`.
pub struct ProcDirNodeOps<D: ProcDirOps> {
    inner: D,
    mode: Mode,
}

/// Convenience alias aligned with Asterinas/Silica conventions.
pub type ProcDir<D> = ProcDirNodeOps<D>;

impl<D: ProcDirOps> ProcDirNodeOps<D> {
    pub fn new(inner: D) -> Self {
        Self {
            inner,
            mode: Mode::DIR
                | Mode::RUSR
                | Mode::XUSR
                | Mode::RGRP
                | Mode::XGRP
                | Mode::ROTH
                | Mode::XOTH,
        }
    }
}

impl<D: ProcDirOps> NodeOps for ProcDirNodeOps<D> {
    fn lookup(&self, name: &str) -> Result<Arc<INode>> {
        self.inner.lookup_child(name)
    }

    fn open(&self, flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        if flags.contains(OpenFlags::WRITE) {
            crate::return_errno!(EISDIR, "cannot write to directory");
        }
        Ok(Box::new(ProcDirFileOps {
            entries: self.inner.list_entries(),
        }))
    }

    fn getattr(&self) -> Result<INodeAttr> {
        Ok(INodeAttr {
            size: 0,
            mode: self.mode,
            nlink: 2,
            uid: 0,
            gid: 0,
            rdev: 0,
        })
    }
}

/// Directory file operations walking in-memory entry names.
pub struct ProcDirFileOps {
    entries: Vec<String>,
}

impl FileOps for ProcDirFileOps {
    fn read(&self, _offset: u64, _buf: &mut [u8]) -> Result<usize> {
        crate::return_errno!(EISDIR, "cannot read raw bytes from directory");
    }

    fn write(&self, _offset: u64, _buf: &[u8]) -> Result<usize> {
        crate::return_errno!(EISDIR, "cannot write to directory");
    }

    fn readdir(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        out.extend_from_slice(b".\n..\n");
        for entry in &self.entries {
            out.extend_from_slice(format!("{}\n", entry).as_bytes());
        }
        Ok(out)
    }
}

/// A declarative static directory table entry.
pub struct StaticDirEntry {
    pub name: &'static str,
    pub ino: u64,
    pub is_dir: bool,
    pub constructor: fn() -> Arc<INode>,
}
