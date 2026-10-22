// SPDX-License-Identifier: GPL-2.0

//! ProcFS Symbolic Link Template and Operations.
//!
//! procfs symlinks are synthesized at `readlink` time from *who* is reading:
//! `/proc/self` resolves to the caller's PID directory, `/proc/<pid>/cwd`
//! may resolve on demand. The [`ProcSymOps::readlink`] operation is
//! therefore the entire symlink implementation; the adapter simply projects
//! it onto the VFS [`NodeOps::readlink`] operation.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;

use crate::{
    fs::{
        template::symlink_mode,
        vfs::{INode, INodeAttr, NodeOps},
    },
    uapi::errno::Result,
};

/// Operations for dynamic procfs symlinks.
pub trait ProcSymOps: Send + Sync + 'static {
    /// Produces the link target at the moment of the call.
    fn readlink(&self) -> Result<String>;
}

/// A node operations adapter wrapping any [`ProcSymOps`].
pub struct ProcSymNodeOps<T: ProcSymOps> {
    inner: T,
}

/// Convenience alias aligned with Asterinas/Silica conventions.
pub type ProcSym<T> = ProcSymNodeOps<T>;

impl<T: ProcSymOps> ProcSymNodeOps<T> {
    /// Wraps the symlink content operations into full node ops.
    pub fn new(inner: T) -> Self {
        Self { inner }
    }
}

impl<T: ProcSymOps> NodeOps for ProcSymNodeOps<T> {
    fn readlink(&self) -> Result<String> {
        self.inner.readlink()
    }

    fn getattr(&self) -> Result<INodeAttr> {
        // st_size of a symlink is the length of its target; resolve it on
        // demand that way `ls -l` prints a plausible length. Missing targets
        // (e.g. for dead PID entries) simply report zero.
        let size = self.inner.readlink().map(|t| t.len()).unwrap_or(0);
        Ok(INodeAttr {
            size,
            mode: symlink_mode(),
            nlink: 1,
            uid: 0,
            gid: 0,
            rdev: 0,
        })
    }
}

/// Builds a fully-formed procfs symbolic-link inode.
pub fn sym_node<T: ProcSymOps>(inner: T, ino: usize) -> Arc<INode> {
    let size = inner.readlink().map(|t| t.len()).unwrap_or(0);
    Arc::new(INode::new_with_attr(
        Box::new(ProcSymNodeOps::new(inner)),
        ino,
        INodeAttr {
            size,
            mode: symlink_mode(),
            nlink: 1,
            uid: 0,
            gid: 0,
            rdev: 0,
        },
    ))
}
