// SPDX-License-Identifier: GPL-2.0

//! DRM card device nodes (`/dev/dri/cardN`).
//!
//! Registers the `dri` device directory against the DRM card registry of
//! [`crate::drivers::drm::core`], so nodes appear as soon as a GPU driver
//! registers its device — no fixed enumeration.
//!
//! The per-open file state lives in [`DrmCardFileOps`]: the ioctl/mmap entry
//! points, plus the grant validation that guards user-space mappings.

use alloc::{boxed::Box, format, sync::Arc, vec::Vec};
use spin::Mutex;

use crate::{
    uapi::errno::{Errno, Result},
    drivers::drm::{
        core as drm_core,
        core::ioctl as drm_ioctl,
        gem::{self, FAKE_OFFSET_BASE},
    },
    fs::{poll::PollEvents, vfs::{FileOps, INode, INodeAttr, Mode, NodeOps, OpenFlags}},
    vm::backing::Backing,
};

/// `/dev/dri` directory node: resolves `cardN` (later `renderDN`) names.
pub struct DriDirNodeOps;

impl NodeOps for DriDirNodeOps {
    fn lookup(&self, name: &str) -> Result<Arc<INode>> {
        let minor = match name.strip_prefix("card") {
            Some(idx) => idx.parse::<u32>().map_err(|_| Errno::ENOENT)?,
            None => crate::return_errno!(ENOENT, "unknown node under /dev/dri"),
        };

        let device = drm_core::get_device(minor)?;
        Ok(Arc::new(INode::new(
            Box::new(CardNodeOps { device }),
            Mode::CHAR
                | Mode::RUSR
                | Mode::WUSR
                | Mode::RGRP
                | Mode::WGRP
                | Mode::ROTH
                | Mode::WOTH,
            226,
        )))
    }

    fn open(&self, _flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        Ok(Box::new(DriDirFileOps))
    }

    fn getattr(&self) -> Result<INodeAttr> {
        Ok(INodeAttr {
            size: 0,
            mode: Mode::DIR
                | Mode::RUSR
                | Mode::WUSR
                | Mode::XUSR
                | Mode::RGRP
                | Mode::XGRP
                | Mode::ROTH
                | Mode::XOTH,
            nlink: 2,
            uid: 0,
            gid: 0,
            rdev: 0,
        })
    }
}

/// Lists the registered card nodes as `cardN` lines.
pub struct DriDirFileOps;

impl FileOps for DriDirFileOps {
    fn read(&self, _offset: u64, _buf: &mut [u8]) -> Result<usize> {
        crate::return_errno!(EISDIR, "cannot read raw bytes from a directory");
    }

    fn write(&self, _offset: u64, _buf: &[u8]) -> Result<usize> {
        crate::return_errno!(EISDIR, "cannot write raw bytes to a directory");
    }

    fn readdir(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        for minor in 0..drm_core::DRM_DEVICES.lock().len() {
            out.extend_from_slice(format!("card{minor}\n").as_bytes());
        }
        Ok(out)
    }
}

/// The card inode: creates a fresh per-open file state.
pub struct CardNodeOps {
    device: Arc<dyn drm_core::DrmDevice>,
}

impl NodeOps for CardNodeOps {
    fn open(&self, _flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        Ok(Box::new(DrmCardFileOps {
            device: Arc::clone(&self.device),
            file: Mutex::new(drm_core::DrmFile::new(Arc::clone(&self.device))),
        }))
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

/// The opened DRM card device (`mmap`/`ioctl` interface, no read/write).
pub struct DrmCardFileOps {
    device: Arc<dyn drm_core::DrmDevice>,
    file: Mutex<drm_core::DrmFile>,
}

impl DrmCardFileOps {
    /// Validates a mapping `[offset, offset+len)` against the GEM grants of
    /// the open file: DRM cards only support page-aligned fake offsets inside
    /// the grant region; everything else is rejected (`EACCES`, like Linux).
    fn granted_object(&self, offset: u64, len: usize) -> Result<Arc<gem::GemObject>> {
        if offset <= FAKE_OFFSET_BASE || offset % 4096 != 0 {
            crate::return_errno!(EACCES, "not a valid buffer mapping offset");
        }
        let file = self.file.lock();
        let Some(object) = file.offsets.get(offset) else {
            crate::return_errno!(EACCES, "no grant for this mapping offset");
        };
        if len > object.size() {
            crate::return_errno!(EINVAL, "mapping extends past the buffer object");
        }
        Ok(object)
    }
}

impl FileOps for DrmCardFileOps {
    fn read(&self, _offset: u64, _buf: &mut [u8]) -> Result<usize> {
        crate::return_errno!(ENODEV, "DRM cards do not support read");
    }

    fn write(&self, _offset: u64, _buf: &[u8]) -> Result<usize> {
        crate::return_errno!(ENODEV, "DRM cards do not support write");
    }

    fn ioctl(&self, cmd: u32, arg: usize) -> Result<usize> {
        let mut file = self.file.lock();
        drm_ioctl::dispatch(&mut file, cmd, arg)
    }

    fn poll(&self) -> PollEvents {
        PollEvents::IN | PollEvents::OUT
    }

    fn mmap(&self, offset: u64, len: usize) -> Result<()> {
        self.granted_object(offset, len)?;
        Ok(())
    }

    fn mmap_backing(&self, offset: u64, len: usize) -> Result<Option<Arc<dyn Backing>>> {
        let object = self.granted_object(offset, len)?;
        Ok(Some(gem::GemBacking::granted(object, offset)))
    }

    fn close(&self) -> Result<()> {
        Ok(())
    }
}
