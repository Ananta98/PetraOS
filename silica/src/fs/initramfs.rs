// SPDX-License-Identifier: GPL-2.0

//! Initramfs CPIO filesystem driver and boot loader integration.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::RwLock;

use crate::api::errno::{Errno, Result};
use crate::fs::vfs::{
    FileOps, FileSystem, INode, INodeAttr, Mode, NodeOps, OpenFlags, SeekAnchor, StatFs,
};
use crate::utils::cpio::{CpioArchive, FileType};

/// File operations for reading from an in-memory initramfs file slice.
pub struct InitramfsFileOps {
    data: &'static [u8],
}

impl FileOps for InitramfsFileOps {
    fn read(&self, offset: u64, buf: &mut [u8]) -> Result<usize> {
        let offset = offset as usize;
        if offset >= self.data.len() {
            return Ok(0);
        }
        let available = &self.data[offset..];
        let copy_len = buf.len().min(available.len());
        buf[..copy_len].copy_from_slice(&available[..copy_len]);
        Ok(copy_len)
    }

    fn write(&self, _offset: u64, _buf: &[u8]) -> Result<usize> {
        crate::return_errno!(EROFS, "initramfs is read-only");
    }

    fn seek(&self, curr_offset: u64, anchor: SeekAnchor) -> Result<u64> {
        let new_offset = match anchor {
            SeekAnchor::Start(pos) => pos as i64,
            SeekAnchor::Current(delta) => curr_offset as i64 + delta,
            SeekAnchor::End(delta) => self.data.len() as i64 + delta,
        };
        if new_offset < 0 {
            crate::return_errno!(EINVAL, "invalid seek position");
        }
        Ok(new_offset as u64)
    }
}

pub struct InitramfsFileNodeOps {
    data: &'static [u8],
}

impl NodeOps for InitramfsFileNodeOps {
    fn open(&self, flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        if flags.contains(OpenFlags::WRITE) {
            crate::return_errno!(EROFS, "initramfs is read-only");
        }
        Ok(Box::new(InitramfsFileOps { data: self.data }))
    }

    fn getattr(&self) -> Result<INodeAttr> {
        Ok(INodeAttr {
            size: self.data.len(),
            mode: Mode::FILE | Mode::RUSR | Mode::RGRP | Mode::ROTH | Mode::XUSR | Mode::XGRP | Mode::XOTH,
            nlink: 1,
            uid: 0,
            gid: 0,
            rdev: 0,
        })
    }
}

pub struct InitramfsSymlinkNodeOps {
    target: &'static str,
}

impl NodeOps for InitramfsSymlinkNodeOps {
    fn readlink(&self) -> Result<String> {
        Ok(String::from(self.target))
    }

    fn getattr(&self) -> Result<INodeAttr> {
        Ok(INodeAttr {
            size: self.target.len(),
            mode: Mode::LINK | Mode::RUSR | Mode::RGRP | Mode::ROTH | Mode::XUSR | Mode::XGRP | Mode::XOTH,
            nlink: 1,
            uid: 0,
            gid: 0,
            rdev: 0,
        })
    }
}

pub struct InitramfsDirFileOps {
    pub entries: Arc<RwLock<BTreeMap<String, Arc<INode>>>>,
}

impl FileOps for InitramfsDirFileOps {
    fn read(&self, _offset: u64, _buf: &mut [u8]) -> Result<usize> {
        crate::return_errno!(EISDIR, "cannot read from directory");
    }

    fn write(&self, _offset: u64, _buf: &[u8]) -> Result<usize> {
        crate::return_errno!(EISDIR, "cannot write to directory");
    }

    fn readdir(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        out.extend_from_slice(b".\n..\n");
        for key in self.entries.read().keys() {
            out.extend_from_slice(key.as_bytes());
            out.push(b'\n');
        }
        Ok(out)
    }
}

pub struct InitramfsDirNodeOps {
    pub entries: Arc<RwLock<BTreeMap<String, Arc<INode>>>>,
}

impl InitramfsDirNodeOps {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }

    pub fn insert(&self, name: String, node: Arc<INode>) {
        self.entries.write().insert(name, node);
    }
}

pub struct InitramfsRamFileOps {
    data: Arc<RwLock<Vec<u8>>>,
}

impl FileOps for InitramfsRamFileOps {
    fn read(&self, offset: u64, buf: &mut [u8]) -> Result<usize> {
        let data = self.data.read();
        let offset = offset as usize;
        if offset >= data.len() {
            return Ok(0);
        }
        let available = &data[offset..];
        let copy_len = buf.len().min(available.len());
        buf[..copy_len].copy_from_slice(&available[..copy_len]);
        Ok(copy_len)
    }

    fn write(&self, offset: u64, buf: &[u8]) -> Result<usize> {
        let mut data = self.data.write();
        let offset = offset as usize;
        if offset + buf.len() > data.len() {
            data.resize(offset + buf.len(), 0);
        }
        data[offset..offset + buf.len()].copy_from_slice(buf);
        Ok(buf.len())
    }

    fn seek(&self, curr_offset: u64, anchor: SeekAnchor) -> Result<u64> {
        let len = self.data.read().len() as i64;
        let new_offset = match anchor {
            SeekAnchor::Start(pos) => pos as i64,
            SeekAnchor::Current(delta) => curr_offset as i64 + delta,
            SeekAnchor::End(delta) => len + delta,
        };
        if new_offset < 0 {
            return Err(Errno::EINVAL);
        }
        Ok(new_offset as u64)
    }
}

pub struct InitramfsRamFileNodeOps {
    data: Arc<RwLock<Vec<u8>>>,
}

impl NodeOps for InitramfsRamFileNodeOps {
    fn open(&self, _flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        Ok(Box::new(InitramfsRamFileOps {
            data: Arc::clone(&self.data),
        }))
    }

    fn getattr(&self) -> Result<INodeAttr> {
        Ok(INodeAttr {
            size: self.data.read().len(),
            mode: Mode::FILE | Mode::RUSR | Mode::WUSR | Mode::RGRP | Mode::ROTH,
            nlink: 1,
            uid: 0,
            gid: 0,
            rdev: 0,
        })
    }

    fn setattr(&self, attr: &INodeAttr) -> Result<()> {
        self.data.write().resize(attr.size, 0);
        Ok(())
    }
}

pub struct InitramfsHeapSymlinkNodeOps {
    target: String,
}

impl NodeOps for InitramfsHeapSymlinkNodeOps {
    fn readlink(&self) -> Result<String> {
        Ok(self.target.clone())
    }

    fn getattr(&self) -> Result<INodeAttr> {
        Ok(INodeAttr {
            size: self.target.len(),
            mode: Mode::LINK | Mode::RUSR | Mode::RGRP | Mode::ROTH | Mode::XUSR | Mode::XGRP | Mode::XOTH,
            nlink: 1,
            uid: 0,
            gid: 0,
            rdev: 0,
        })
    }
}

impl NodeOps for InitramfsDirNodeOps {
    fn lookup(&self, name: &str) -> Result<Arc<INode>> {
        self.entries.read().get(name).cloned().ok_or(Errno::ENOENT)
    }

    fn open(&self, flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        if flags.contains(OpenFlags::WRITE) {
            crate::return_errno!(EISDIR, "cannot write to directory");
        }
        Ok(Box::new(InitramfsDirFileOps {
            entries: Arc::clone(&self.entries),
        }))
    }

    fn create(&self, name: &str, mode: Mode) -> Result<Arc<INode>> {
        static NEXT_INO: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(200_000);
        let ino = NEXT_INO.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        let data = Arc::new(RwLock::new(Vec::new()));
        let node = Arc::new(INode::new_with_attr(
            Box::new(InitramfsRamFileNodeOps { data }),
            ino,
            INodeAttr {
                size: 0,
                mode,
                nlink: 1,
                uid: 0,
                gid: 0,
                rdev: 0,
            },
        ));
        self.entries.write().insert(String::from(name), node.clone());
        Ok(node)
    }

    fn mkdir(&self, name: &str, mode: Mode) -> Result<Arc<INode>> {
        static NEXT_INO: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(200_000);
        let ino = NEXT_INO.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        let dir_entries = Arc::new(RwLock::new(BTreeMap::new()));
        let node = Arc::new(INode::new(
            Box::new(InitramfsDirNodeOps {
                entries: dir_entries,
            }),
            mode,
            ino,
        ));
        self.entries.write().insert(String::from(name), node.clone());
        Ok(node)
    }

    fn unlink(&self, name: &str) -> Result<()> {
        self.entries.write().remove(name).ok_or(Errno::ENOENT)?;
        Ok(())
    }

    fn rmdir(&self, name: &str) -> Result<()> {
        let mut entries = self.entries.write();
        let child = entries.get(name).ok_or(Errno::ENOENT)?;
        if !child.is_dir() {
            return Err(Errno::ENOTDIR);
        }
        entries.remove(name);
        Ok(())
    }

    fn symlink(&self, name: &str, target: &str) -> Result<Arc<INode>> {
        static NEXT_INO: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(200_000);
        let ino = NEXT_INO.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        let node = Arc::new(INode::new_with_attr(
            Box::new(InitramfsHeapSymlinkNodeOps {
                target: String::from(target),
            }),
            ino,
            INodeAttr {
                size: target.len(),
                mode: Mode::LINK | Mode::from_bits_truncate(0o777),
                nlink: 1,
                uid: 0,
                gid: 0,
                rdev: 0,
            },
        ));
        self.entries.write().insert(String::from(name), node.clone());
        Ok(node)
    }

    fn link(&self, name: &str, other: Arc<INode>) -> Result<()> {
        self.entries.write().insert(String::from(name), other);
        Ok(())
    }

    fn mknod(&self, name: &str, mode: Mode, rdev: u64) -> Result<Arc<INode>> {
        static NEXT_INO: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(200_000);
        let ino = NEXT_INO.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        let node = Arc::new(INode::new_with_attr(
            Box::new(InitramfsFileNodeOps { data: &[] }),
            ino,
            INodeAttr {
                size: 0,
                mode,
                nlink: 1,
                uid: 0,
                gid: 0,
                rdev,
            },
        ));
        self.entries.write().insert(String::from(name), node.clone());
        Ok(node)
    }
}

/// The read-only CPIO initramfs filesystem instance.
pub struct InitramfsFs {
    root: Arc<INode>,
}

impl InitramfsFs {
    /// Creates and populates an Initramfs filesystem from boot memory.
    pub fn new() -> Self {
        let root_entries = Arc::new(RwLock::new(BTreeMap::new()));
        let root = Arc::new(INode::new(
            Box::new(InitramfsDirNodeOps {
                entries: Arc::clone(&root_entries),
            }),
            Mode::DIR | Mode::RUSR | Mode::WUSR | Mode::XUSR | Mode::RGRP | Mode::XGRP | Mode::ROTH | Mode::XOTH,
            1,
        ));

        let boot_info = ostd::boot::boot_info();
        if let Some(initramfs_buf) = boot_info.initramfs {
            ostd::info!("Building VFS tree from initramfs ({} bytes)...", initramfs_buf.len());
            let archive = CpioArchive::new(initramfs_buf);
            let mut ino = 2;
            let mut count = 0;

            // Maps directory path -> directory entries map
            let mut dir_map: BTreeMap<String, Arc<RwLock<BTreeMap<String, Arc<INode>>>>> = BTreeMap::new();
            dir_map.insert(String::new(), Arc::clone(&root_entries));

            for entry in archive.flatten() {
                let clean = entry.name.trim_start_matches('.').trim_start_matches('/');
                if clean.is_empty() {
                    continue;
                }

                let parts: Vec<&str> = clean.split('/').filter(|s| !s.is_empty()).collect();
                if parts.is_empty() {
                    continue;
                }

                // Ensure intermediate directories exist
                let mut current_path = String::new();
                for i in 0..parts.len() - 1 {
                    let parent_path = current_path.clone();
                    if !current_path.is_empty() {
                        current_path.push('/');
                    }
                    current_path.push_str(parts[i]);

                    if !dir_map.contains_key(&current_path) {
                        let new_dir_entries = Arc::new(RwLock::new(BTreeMap::new()));
                        let new_dir_node = Arc::new(INode::new(
                            Box::new(InitramfsDirNodeOps {
                                entries: Arc::clone(&new_dir_entries),
                            }),
                            Mode::DIR | Mode::RUSR | Mode::WUSR | Mode::XUSR | Mode::RGRP | Mode::XGRP | Mode::ROTH | Mode::XOTH,
                            ino,
                        ));
                        ino += 1;

                        if let Some(parent_entries) = dir_map.get(&parent_path) {
                            parent_entries.write().insert(parts[i].into(), new_dir_node);
                        }
                        dir_map.insert(current_path.clone(), new_dir_entries);
                    }
                }

                let parent_path = if parts.len() > 1 {
                    parts[..parts.len() - 1].join("/")
                } else {
                    String::new()
                };

                let leaf_name = parts.last().unwrap();
                match entry.file_type {
                    FileType::Directory => {
                        let mut full_dir_path = parent_path.clone();
                        if !full_dir_path.is_empty() {
                            full_dir_path.push('/');
                        }
                        full_dir_path.push_str(leaf_name);

                        if !dir_map.contains_key(&full_dir_path) {
                            let new_dir_entries = Arc::new(RwLock::new(BTreeMap::new()));
                            let new_dir_node = Arc::new(INode::new(
                                Box::new(InitramfsDirNodeOps {
                                    entries: Arc::clone(&new_dir_entries),
                                }),
                                Mode::DIR | Mode::RUSR | Mode::WUSR | Mode::XUSR | Mode::RGRP | Mode::XGRP | Mode::ROTH | Mode::XOTH,
                                ino,
                            ));
                            ino += 1;

                            if let Some(parent_entries) = dir_map.get(&parent_path) {
                                parent_entries.write().insert((*leaf_name).into(), new_dir_node);
                            }
                            dir_map.insert(full_dir_path, new_dir_entries);
                        }
                    }
                    FileType::Symlink => {
                        let target_str = core::str::from_utf8(entry.data).unwrap_or("");
                        let symlink_node = Arc::new(INode::new_with_attr(
                            Box::new(InitramfsSymlinkNodeOps { target: target_str }),
                            ino,
                            INodeAttr {
                                size: target_str.len(),
                                mode: Mode::LINK | Mode::RUSR | Mode::RGRP | Mode::ROTH | Mode::XUSR | Mode::XGRP | Mode::XOTH,
                                nlink: 1,
                                uid: entry.uid,
                                gid: entry.gid,
                                rdev: 0,
                            },
                        ));
                        ino += 1;
                        count += 1;

                        if let Some(parent_entries) = dir_map.get(&parent_path) {
                            parent_entries.write().insert((*leaf_name).into(), symlink_node);
                        }
                    }
                    _ => {
                        let file_node = Arc::new(INode::new_with_attr(
                            Box::new(InitramfsFileNodeOps { data: entry.data }),
                            ino,
                            INodeAttr {
                                size: entry.data.len(),
                                mode: Mode::FILE | Mode::RUSR | Mode::WUSR | Mode::XUSR | Mode::RGRP | Mode::XGRP | Mode::ROTH | Mode::XOTH,
                                nlink: 1,
                                uid: entry.uid,
                                gid: entry.gid,
                                rdev: 0,
                            },
                        ));
                        ino += 1;
                        count += 1;

                        if let Some(parent_entries) = dir_map.get(&parent_path) {
                            parent_entries.write().insert((*leaf_name).into(), file_node);
                        }
                    }
                }
            }
            ostd::info!("Initramfs VFS tree mounted: {} files loaded", count);
        } else {
            ostd::warn!("No initramfs provided by bootloader");
        }

        Self { root }
    }
}

impl FileSystem for InitramfsFs {
    fn name(&self) -> &'static str {
        "initramfs"
    }

    fn root(&self) -> Result<Arc<INode>> {
        Ok(Arc::clone(&self.root))
    }

    fn statfs(&self) -> Result<StatFs> {
        Ok(StatFs {
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

/// Reads file data from initramfs by path (legacy compatibility helper).
pub fn read_file_from_initramfs(path: &str) -> Option<&'static [u8]> {
    let clean = path.trim_start_matches('/');
    let initramfs_buf = ostd::boot::boot_info().initramfs?;
    let archive = CpioArchive::new(initramfs_buf);
    for entry in archive.flatten() {
        let name = entry.name.trim_start_matches('.').trim_start_matches('/');
        if name == clean {
            return Some(entry.data);
        }
    }
    None
}