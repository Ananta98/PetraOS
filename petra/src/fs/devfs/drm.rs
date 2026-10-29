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
        if name == "." || name == ".." {
            return Ok(Arc::new(INode::new(
                Box::new(DriDirNodeOps),
                Mode::DIR
                    | Mode::RUSR
                    | Mode::WUSR
                    | Mode::XUSR
                    | Mode::RGRP
                    | Mode::XGRP
                    | Mode::ROTH
                    | Mode::XOTH,
                12,
            )));
        }

        let (minor, rdev) = if let Some(idx) = name.strip_prefix("card") {
            let m = idx.parse::<u32>().map_err(|_| Errno::ENOENT)?;
            (m, ((226 as u64) << 8) | (m as u64))
        } else if let Some(idx) = name.strip_prefix("renderD") {
            let r = idx.parse::<u32>().map_err(|_| Errno::ENOENT)?;
            (r.saturating_sub(128), ((226 as u64) << 8) | (r as u64))
        } else {
            return Err(Errno::ENOENT);
        };

        let device = drm_core::get_device(minor)?;
        Ok(Arc::new(INode::new(
            Box::new(CardNodeOps { device, rdev }),
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
        out.extend_from_slice(b".\n..\n");
        for minor in 0..drm_core::DRM_DEVICES.lock().len() {
            out.extend_from_slice(format!("card{minor}\n").as_bytes());
            out.extend_from_slice(format!("renderD{}\n", 128 + minor).as_bytes());
        }
        Ok(out)
    }
}

/// The card inode: creates a fresh per-open file state.
pub struct CardNodeOps {
    device: Arc<dyn drm_core::DrmDevice>,
    rdev: u64,
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
            rdev: self.rdev,
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
    fn read(&self, _offset: u64, buf: &mut [u8]) -> Result<usize> {
        let event_size = core::mem::size_of::<crate::uapi::drm::DrmEventVblank>();
        if buf.len() < event_size {
            crate::return_errno!(EINVAL, "buffer too small for DRM event");
        }
        let file = self.file.lock();
        let mut events = file.events.lock();
        let mut read_bytes = 0;
        while buf.len() - read_bytes >= event_size {
            if let Some(ev) = events.pop_front() {
                let slice: &[u8] = zerocopy::IntoBytes::as_bytes(&ev);
                buf[read_bytes..read_bytes + event_size].copy_from_slice(slice);
                read_bytes += event_size;
            } else {
                break;
            }
        }
        if read_bytes > 0 {
            Ok(read_bytes)
        } else {
            Err(Errno::EAGAIN)
        }
    }

    fn write(&self, _offset: u64, _buf: &[u8]) -> Result<usize> {
        crate::return_errno!(ENODEV, "DRM cards do not support write");
    }

    fn ioctl(&self, cmd: u32, arg: usize) -> Result<usize> {
        let mut file = self.file.lock();
        drm_ioctl::dispatch(&mut file, cmd, arg)
    }

    fn poll(&self) -> PollEvents {
        let file = self.file.lock();
        let has_events = !file.events.lock().is_empty();
        if has_events {
            PollEvents::IN | PollEvents::RDNORM | PollEvents::OUT | PollEvents::WRNORM
        } else {
            PollEvents::OUT | PollEvents::WRNORM
        }
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
