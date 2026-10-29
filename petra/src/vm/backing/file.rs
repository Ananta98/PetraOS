// SPDX-License-Identifier: GPL-2.0

//! File-backed virtual memory implementation (`mmap(2)`).

use alloc::{collections::BTreeMap, sync::Arc};
use core::{
    fmt::{self, Debug, Formatter},
    ops::Range,
};
use ostd::{
    mm::{PAGE_SIZE, UFrame, io::util::HasVmReaderWriter},
    sync::SpinLock,
};

use super::{Backing, alloc_zeroed_frame};
use crate::{uapi::errno::Result, fs::File};

/// A backing object for file-mapped virtual memory (`mmap(2)`).
///
/// Pages are populated on-demand on page faults by reading directly from the underlying
/// file. For shared writable mappings (`MAP_SHARED`), modified pages are flushed
/// back to storage via `flush` (`msync(2)`).
pub struct FileBacking {
    file: Arc<File>,
    size: usize,
    /// Materialized frames keyed by their page-aligned offset in the file.
    pages: SpinLock<BTreeMap<usize, UFrame>>,
}

impl FileBacking {
    /// Creates a new `FileBacking` for the given open file description.
    pub fn new(file: Arc<File>) -> Result<Self> {
        Self::new_with_min_size(file, 0)
    }

    /// Creates a new `FileBacking` ensuring the backing size covers at least `min_size` bytes.
    pub fn new_with_min_size(file: Arc<File>, min_size: usize) -> Result<Self> {
        let raw_size = (file.inode.size() as usize).max(min_size);
        // Page-align the backing size so partial trailing pages can be mapped without failure
        let size = (raw_size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        Ok(Self {
            file,
            size,
            pages: SpinLock::new(BTreeMap::new()),
        })
    }

    /// Returns the underlying file handle.
    pub fn file(&self) -> &Arc<File> {
        &self.file
    }

    /// Returns the page at `offset`, reading from the file if this is its first use.
    fn frame_at(&self, offset: usize) -> Result<UFrame> {
        if offset >= self.size {
            crate::return_errno!(
                EINVAL,
                "offset {offset:#x} is past the end of file ({:#x})",
                self.size
            );
        }

        let mut pages = self.pages.lock();
        if let Some(frame) = pages.get(&offset) {
            return Ok(frame.clone());
        }

        // Allocate a zeroed physical frame
        let frame = alloc_zeroed_frame()?;

        // Read up to PAGE_SIZE bytes from the file at `offset`
        let mut buf = [0u8; PAGE_SIZE];
        let n = self.file.ops.fault_in(offset as u64, &mut buf).unwrap_or(0);

        if n > 0 {
            let mut writer = frame.writer();
            let mut reader = ostd::mm::VmReader::from(&buf[..n]);
            writer.write(&mut reader);
        }

        pages.insert(offset, frame.clone());
        Ok(frame)
    }
}

impl Backing for FileBacking {
    fn size(&self) -> usize {
        self.size
    }

    fn frame(&self, offset: usize) -> Result<UFrame> {
        self.frame_at(offset)
    }

    fn flush(&self, range: Range<usize>) -> Result<()> {
        let pages = self.pages.lock();
        let file_len = self.file.inode.size().max(self.size);
        for (&offset, frame) in pages.range(range.start..range.end) {
            if offset >= file_len {
                continue;
            }
            let mut reader = frame.reader();
            let mut buf = [0u8; PAGE_SIZE];
            let mut writer = ostd::mm::VmWriter::from(&mut buf[..]);
            reader.read(&mut writer);
            let write_len = PAGE_SIZE.min(file_len.saturating_sub(offset));
            if write_len > 0 {
                let _ = self.file.ops.write(offset as u64, &buf[..write_len]);
                let new_size = offset + write_len;
                if new_size > self.file.inode.size() {
                    self.file.inode.set_size(new_size);
                }
            }
        }
        self.file.inode.touch_mtime();
        let _ = self.file.ops.fsync(false);
        Ok(())
    }
}

impl Drop for FileBacking {
    fn drop(&mut self) {
        let _ = self.flush(0..self.size);
    }
}

impl Debug for FileBacking {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let resident = self.pages.lock().len();
        f.debug_struct("FileBacking")
            .field("size", &self.size)
            .field("resident_pages", &resident)
            .finish()
    }
}
