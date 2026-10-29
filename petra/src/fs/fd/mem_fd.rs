// SPDX-License-Identifier: GPL-2.0

//! Anonymous in-memory file descriptors (`memfd_create(2)`).
//!
//! Provides anonymous memory-backed file descriptions capable of sealing,
//! memory mapping (`mmap`), and IPC transfers across process boundaries.

use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicU32, Ordering};
use spin::RwLock;

use crate::{
    fs::{
        poll::PollEvents,
        vfs::{File, FileOps, INode, INodeAttr, Mode, NodeOps, OpenFlags, SeekAnchor},
    },
    uapi::{
        errno::{Errno, Result},
        fd::{
            F_SEAL_GROW, F_SEAL_SEAL, F_SEAL_SHRINK, F_SEAL_WRITE, MFD_ALLOW_SEALING,
        },
    },
};

pub use crate::uapi::fd::{
    F_SEAL_FUTURE_WRITE, MFD_CLOEXEC, MFD_HUGETLB, MFD_HUGE_MASK, MFD_HUGE_SHIFT,
    MFD_KNOWN_FLAGS, MFD_NAME_MAX_LEN,
};

const MEMFD_INODE_NUMBER: usize = 902;

/// Anonymous memory-backed descriptor state.
pub struct MemFd {
    name: String,
    buffer: Arc<RwLock<Vec<u8>>>,
    seals: AtomicU32,
    allow_sealing: bool,
}

impl MemFd {
    pub fn new(name: String, allow_sealing: bool) -> Self {
        Self {
            name,
            buffer: Arc::new(RwLock::new(Vec::new())),
            seals: AtomicU32::new(0),
            allow_sealing,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn get_seals(&self) -> u32 {
        self.seals.load(Ordering::Acquire)
    }

    pub fn add_seals(&self, new_seals: u32) -> Result<()> {
        if !self.allow_sealing {
            return Err(Errno::EPERM);
        }
        let current = self.seals.load(Ordering::Acquire);
        if (current & F_SEAL_SEAL) != 0 {
            return Err(Errno::EPERM);
        }
        self.seals.fetch_or(new_seals, Ordering::Release);
        Ok(())
    }

    pub fn read_bytes(&self, offset: u64, dest_buffer: &mut [u8]) -> usize {
        let storage = self.buffer.read();
        let start_pos = offset as usize;
        if start_pos >= storage.len() {
            return 0;
        }
        let available = &storage[start_pos..];
        let bytes_to_copy = dest_buffer.len().min(available.len());
        dest_buffer[..bytes_to_copy].copy_from_slice(&available[..bytes_to_copy]);
        bytes_to_copy
    }

    pub fn write_bytes(&self, offset: u64, src_buffer: &[u8]) -> Result<usize> {
        let seals = self.seals.load(Ordering::Acquire);
        if (seals & F_SEAL_WRITE) != 0 {
            return Err(Errno::EPERM);
        }

        let start_pos = offset as usize;
        let end_pos = start_pos.saturating_add(src_buffer.len());
        let mut storage = self.buffer.write();

        if end_pos > storage.len() {
            if (seals & F_SEAL_GROW) != 0 {
                return Err(Errno::EPERM);
            }
            storage.resize(end_pos, 0);
        }

        storage[start_pos..end_pos].copy_from_slice(src_buffer);
        Ok(src_buffer.len())
    }

    pub fn truncate(&self, target_size: usize) -> Result<()> {
        let seals = self.seals.load(Ordering::Acquire);
        let mut storage = self.buffer.write();
        let current_size = storage.len();

        if target_size < current_size && (seals & F_SEAL_SHRINK) != 0 {
            return Err(Errno::EPERM);
        }
        if target_size > current_size && (seals & F_SEAL_GROW) != 0 {
            return Err(Errno::EPERM);
        }

        storage.resize(target_size, 0);
        Ok(())
    }
}

pub struct MemFdFileOps {
    instance: Arc<MemFd>,
}

impl FileOps for MemFdFileOps {
    fn read(&self, offset: u64, buf: &mut [u8]) -> Result<usize> {
        Ok(self.instance.read_bytes(offset, buf))
    }

    fn write(&self, offset: u64, buf: &[u8]) -> Result<usize> {
        self.instance.write_bytes(offset, buf)
    }

    fn seek(&self, curr_offset: u64, anchor: SeekAnchor) -> Result<u64> {
        let current_len = self.instance.buffer.read().len() as i64;
        let target_offset = match anchor {
            SeekAnchor::Start(position) => position as i64,
            SeekAnchor::Current(delta) => curr_offset as i64 + delta,
            SeekAnchor::End(delta) => current_len + delta,
        };
        if target_offset < 0 {
            return Err(Errno::EINVAL);
        }
        Ok(target_offset as u64)
    }

    fn poll(&self) -> PollEvents {
        PollEvents::IN | PollEvents::OUT | PollEvents::RDNORM | PollEvents::WRNORM
    }

    fn mmap(&self, _offset: u64, _len: usize) -> Result<()> {
        Ok(())
    }

    fn fault_in(&self, offset: u64, buf: &mut [u8]) -> Result<usize> {
        Ok(self.instance.read_bytes(offset, buf))
    }

    fn as_memfd(&self) -> Option<Arc<MemFd>> {
        Some(Arc::clone(&self.instance))
    }
}

pub struct MemFdNodeOps {
    instance: Arc<MemFd>,
}

impl NodeOps for MemFdNodeOps {
    fn open(&self, _flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        Ok(Box::new(MemFdFileOps {
            instance: Arc::clone(&self.instance),
        }))
    }

    fn getattr(&self) -> Result<INodeAttr> {
        let size = self.instance.buffer.read().len();
        Ok(INodeAttr {
            size,
            mode: Mode::FILE | Mode::RUSR | Mode::WUSR | Mode::RGRP | Mode::WGRP,
            nlink: 1,
            uid: 0,
            gid: 0,
            rdev: 0,
        })
    }

    fn setattr(&self, attr: &INodeAttr) -> Result<()> {
        self.instance.truncate(attr.size)
    }
}

/// Creates an open `File` referencing an anonymous memory buffer.
pub fn create_memfd_file(name: String, flags: u32) -> Result<Arc<File>> {
    let allow_sealing = (flags & MFD_ALLOW_SEALING) != 0;
    let memfd = Arc::new(MemFd::new(name, allow_sealing));

    let inode = Arc::new(INode::new(
        Box::new(MemFdNodeOps {
            instance: Arc::clone(&memfd),
        }),
        Mode::FILE | Mode::RUSR | Mode::WUSR | Mode::RGRP | Mode::WGRP,
        MEMFD_INODE_NUMBER,
    ));

    Ok(Arc::new(File::new(
        Box::new(MemFdFileOps { instance: memfd }),
        inode,
        OpenFlags::READ | OpenFlags::WRITE,
        true,
    )))
}
