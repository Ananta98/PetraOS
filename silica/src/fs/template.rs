// SPDX-License-Identifier: GPL-2.0

//! Shared building blocks for in-memory pseudo-filesystems.
//!
//! `procfs` and `sysfs` never touch storage: every node is synthesized from
//! kernel state on access. Both of them therefore share the same boring
//! mechanics — a read/seek implementation over an in-memory byte buffer, an
//! optional write hook, and the standard file-mode bit patterns — and this
//! module keeps a single copy of each.
//!
//! The layout mirrors the spirit of Linux `fs/libfs.c`, which hosts the
//! equivalent plumbing (`simple_read`, `simple_write`, `simple_seek`) that
//! pseudo-filesystems build upon.

use alloc::{boxed::Box, vec::Vec};

use crate::{
    api::errno::{Errno, Result},
    fs::vfs::{FileOps, Mode, SeekAnchor},
};

/// A write callback that receives the raw user buffer and returns the number
/// of consumed bytes.
pub type WriteHook = Box<dyn Fn(&[u8]) -> Result<usize> + Send + Sync>;

/// File operations backed by a plain in-memory byte buffer.
///
/// The buffer holds the content *as of the last open*: for pseudo-files whose
/// content is regenerated per open (procfs) or per attribute read (sysfs),
/// writes go through the optional hook while reads come from the snapshot.
pub struct BufFileOps {
    data: Vec<u8>,
    write_hook: Option<WriteHook>,
}

impl BufFileOps {
    /// Creates read-only operations over `data`.
    pub fn new(data: Vec<u8>) -> Self {
        Self {
            data,
            write_hook: None,
        }
    }

    /// Creates operations over `data` that additionally accept writes by
    /// forwarding them to `hook`.
    pub fn with_write_hook(data: Vec<u8>, hook: WriteHook) -> Self {
        Self {
            data,
            write_hook: Some(hook),
        }
    }
}

impl FileOps for BufFileOps {
    fn read(&self, offset: u64, buf: &mut [u8]) -> Result<usize> {
        let offset = usize::try_from(offset).unwrap_or(usize::MAX);
        if offset >= self.data.len() {
            return Ok(0);
        }
        let available = &self.data[offset..];
        let copy_len = buf.len().min(available.len());
        buf[..copy_len].copy_from_slice(&available[..copy_len]);
        Ok(copy_len)
    }

    fn write(&self, _offset: u64, buf: &[u8]) -> Result<usize> {
        match &self.write_hook {
            Some(hook) => hook(buf),
            None => crate::return_errno!(EROFS, "file is read-only"),
        }
    }

    fn seek(&self, curr_offset: u64, anchor: SeekAnchor) -> Result<u64> {
        let len = self.data.len() as i64;
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

/// Creates read-only file operations over `data` with no write support.
pub fn buf_file_ops(data: Vec<u8>) -> Box<dyn FileOps> {
    Box::new(BufFileOps::new(data))
}

/// Creates file operations over `data` that forward writes to `hook`.
pub fn writeable_buf_file_ops(data: Vec<u8>, hook: WriteHook) -> Box<dyn FileOps> {
    Box::new(BufFileOps::with_write_hook(data, hook))
}

/// Mode of a pseudo-directory: world readable and searchable, `0755` without
/// owner write.
pub fn dir_mode() -> Mode {
    Mode::DIR | Mode::RUSR | Mode::XUSR | Mode::RGRP | Mode::XGRP | Mode::ROTH | Mode::XOTH
}

/// Mode of a pseudo-file readable by everyone.
pub fn ro_file_mode() -> Mode {
    Mode::FILE | Mode::RUSR | Mode::RGRP | Mode::ROTH
}

/// Mode of a pseudo-file that users may also write (sysctls, sysfs attrs).
pub fn rw_file_mode() -> Mode {
    Mode::FILE | Mode::RUSR | Mode::WUSR | Mode::RGRP | Mode::ROTH
}

/// Mode of a pseudo-symbolic-link (target-following, `0777`).
pub fn symlink_mode() -> Mode {
    Mode::LINK | Mode::from_bits_truncate(0o777)
}
