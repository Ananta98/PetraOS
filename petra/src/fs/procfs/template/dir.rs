// SPDX-License-Identifier: GPL-2.0

//! ProcFS Directory Template and Operations.
//!
//! Directories are the *organizing* half of procfs: they contain no state of
//! their own and simply forward `lookup` to per-directory logic while
//! centralizing the tedious parts — rejecting access to raw bytes of a
//! directory, building `readdir` listings, and stamping uniform directory
//! attributes.
//!
//! Static directories are additionally described by declarative tables of
//! [`StaticDirEntry`]; the same table then drives both name resolution
//! (through [`lookup_static_entry`]) and enumeration (through
//! [`list_static_entries`]), so the two can never drift apart. This follows
//! the upstream Asterinas kernel, whose `RootDirOps::STATIC_ENTRIES` table is
//! consumed by `lookup_child_from_table` and `listed_entries_from_table`
//! helpers.

use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};

use crate::{
    fs::{
        template::dir_mode,
        vfs::{FileOps, INode, INodeAttr, NodeOps, OpenFlags},
    },
    uapi::errno::Result,
};

/// Content operations for a procfs directory, implemented per directory.
pub trait ProcDirOps: Send + Sync + 'static {
    /// Resolves a child entry by name, returning `ENOENT` for unknown names.
    fn lookup_child(&self, name: &str) -> Result<Arc<INode>>;

    /// Enumerates the entry names of this directory for `readdir(3)`.
    fn list_entries(&self) -> Vec<String>;
}

/// Node operations adapter wrapping any [`ProcDirOps`].
pub struct ProcDirNodeOps<D: ProcDirOps> {
    inner: D,
}

/// Convenience alias aligned with Asterinas conventions.
pub type ProcDir<D> = ProcDirNodeOps<D>;

impl<D: ProcDirOps> ProcDirNodeOps<D> {
    /// Wraps a content-only directory implementation into full node ops.
    pub fn new(inner: D) -> Self {
        Self { inner }
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
        Ok(Box::new(ProcDirFileOps::new(self.inner.list_entries())))
    }

    fn getattr(&self) -> Result<INodeAttr> {
        Ok(dir_attr())
    }
}

/// Attributes shared by every procfs directory.
pub fn dir_attr() -> INodeAttr {
    INodeAttr {
        mode: dir_mode(),
        nlink: 2,
        ..Default::default()
    }
}

/// Directory file operations walking in-memory entry names.
///
/// The listing is snapshotted at open time — the moment the VFS calls
/// `open(2)` on the directory — which is when the per-directory entry set
/// (e.g. the live PID list) is captured.
pub struct ProcDirFileOps {
    entries: Vec<String>,
}

impl ProcDirFileOps {
    /// Creates directory operations over a precomputed listing.
    pub fn new(entries: Vec<String>) -> Self {
        Self { entries }
    }
}

impl FileOps for ProcDirFileOps {
    fn read(&self, _offset: u64, _buf: &mut [u8]) -> Result<usize> {
        crate::return_errno!(EISDIR, "cannot read raw bytes from directory");
    }

    fn write(&self, _offset: u64, _buf: &[u8]) -> Result<usize> {
        crate::return_errno!(EISDIR, "cannot write to directory");
    }

    fn readdir(&self) -> Result<Vec<u8>> {
        let mut out = Vec::from(b".\n..\n");
        for entry in &self.entries {
            out.extend_from_slice(entry.as_bytes());
            out.push(b'\n');
        }
        Ok(out)
    }
}

/// A declarative static directory table entry.
///
/// The constructor produces a fully-formed VFS inode (node ops, mode and
/// inode number all included), mirroring the `("name", ctor)` tuple rows of
/// the Asterinas procfs reference.
pub struct StaticDirEntry {
    pub name: &'static str,
    pub ctor: fn() -> Arc<INode>,
}

/// Resolves `name` against a static directory table.
pub fn lookup_static_entry(table: &'static [StaticDirEntry], name: &str) -> Option<Arc<INode>> {
    table.iter().find(|e| e.name == name).map(|e| (e.ctor)())
}

/// Enumerates the names of a static directory table for `readdir`.
pub fn list_static_entries(table: &'static [StaticDirEntry]) -> Vec<String> {
    table.iter().map(|e| String::from(e.name)).collect()
}

/// Builds a fully-formed procfs directory inode.
pub fn dir_node<D: ProcDirOps>(inner: D, ino: usize) -> Arc<INode> {
    Arc::new(INode::new_with_attr(
        Box::new(ProcDirNodeOps::new(inner)),
        ino,
        dir_attr(),
    ))
}
