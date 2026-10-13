// SPDX-License-Identifier: GPL-2.0

//! ProcFS Symbolic Link Template and Operations.

use alloc::string::String;

use crate::{
    api::errno::Result,
    fs::vfs::{INodeAttr, Mode, NodeOps},
};

/// Operations for dynamic procfs symlinks.
pub trait ProcSymOps: Send + Sync + 'static {
    fn readlink(&self) -> Result<String>;
}

/// A node operations adapter wrapping any `ProcSymOps`.
pub struct ProcSymNodeOps<T: ProcSymOps> {
    inner: T,
}

/// Convenience alias aligned with Asterinas/Silica conventions.
pub type ProcSym<T> = ProcSymNodeOps<T>;

impl<T: ProcSymOps> ProcSymNodeOps<T> {
    pub fn new(inner: T) -> Self {
        Self { inner }
    }
}

impl<T: ProcSymOps> NodeOps for ProcSymNodeOps<T> {
    fn readlink(&self) -> Result<String> {
        self.inner.readlink()
    }

    fn getattr(&self) -> Result<INodeAttr> {
        Ok(INodeAttr {
            size: 1,
            mode: Mode::LINK | Mode::from_bits_truncate(0o777),
            nlink: 1,
            uid: 0,
            gid: 0,
            rdev: 0,
        })
    }
}
