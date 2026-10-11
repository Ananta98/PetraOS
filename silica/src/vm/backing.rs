// SPDX-License-Identifier: GPL-2.0

//! The objects that supply physical pages to mappings.
//!
//! A mapping either has no backing object at all, in which case it is
//! `MAP_PRIVATE` anonymous memory and every fault allocates a fresh zeroed
//! page, or it refers to a [`Backing`], in which case the frames the mapping
//! installs come from that object and are shared with every other mapping of
//! it.
//!
//! Only `size` and `frame` have to be implemented. Ranges are populated by
//! driving the ordinary page-fault path (see `Vmar::populate_range`), so a
//! backing never needs to know how to batch or pre-fault; and `flush` has nothing
//! to do for an object that is already memory, so it defaults to doing nothing.
//! A file-backed implementation overrides `flush` to write out, and can add a
//! readahead method without disturbing either.

use alloc::{
    collections::BTreeMap,
    sync::Arc,
};
use core::{fmt::{self, Debug, Formatter}, ops::Range};
use ostd::{
    mm::{FrameAllocOptions, PAGE_SIZE, UFrame, io::util::HasVmReaderWriter},
    sync::SpinLock,
};

use crate::{
    api::errno::Result,
    fs::File,
};

/// A source of physical pages for a mapping.
///
/// Implementations must be safe to call concurrently from page-fault handlers
/// on different CPUs, and `frame` must return the *same* page for the same
/// offset every time so that two mappings of one object alias.
pub trait Backing: Send + Sync + Debug {
    /// Returns the size of the object in bytes.
    ///
    /// A mapping may be larger than its backing, in which case the pages past
    /// the end are not backed by the object.
    fn size(&self) -> usize;

    /// Returns the page that backs `offset`, materializing it if needed.
    ///
    /// `offset` is always a multiple of the page size and less than
    /// [`Self::size`].
    fn frame(&self, offset: usize) -> Result<UFrame>;

    /// Makes every change to `range` visible to a later read of the object,
    /// whether through this mapping or any other.
    ///
    /// This is what `msync(2)` is for, and it is where a file-backed object
    /// writes its pages out. An object with nowhere to push to, such as plain
    /// memory, does nothing: its pages are already the one true copy, so there
    /// is no separate copy to bring up to date.
    ///
    /// `range` is page-aligned and lies within [`Self::size`]. An implementation
    /// must be safe to call from a task that is not holding any lock on this
    /// object.
    fn flush(&self, _range: Range<usize>) -> Result<()> {
        Ok(())
    }
}

/// Allocates a single zero-filled page from the physical memory allocator.
pub fn alloc_zeroed_frame() -> Result<UFrame> {
    Ok(FrameAllocOptions::new().alloc_frame()?.into())
}

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
        let file_len = self.file.inode.size();
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
            }
        }
        let _ = self.file.ops.fsync(false);
        Ok(())
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

#[cfg(ktest)]
mod tests {
    use ostd::prelude::ktest;
    use super::*;
    use alloc::{boxed::Box, sync::Arc, vec::Vec};
    use spin::RwLock;
    use crate::fs::vfs::{FileOps, INode, INodeAttr, Mode, OpenFlags};

    struct TestFileOps {
        data: Arc<RwLock<Vec<u8>>>,
    }

    impl FileOps for TestFileOps {
        fn read(&self, offset: u64, buf: &mut [u8]) -> Result<usize> {
            let data = self.data.read();
            let off = offset as usize;
            if off >= data.len() {
                return Ok(0);
            }
            let avail = &data[off..];
            let n = buf.len().min(avail.len());
            buf[..n].copy_from_slice(&avail[..n]);
            Ok(n)
        }

        fn write(&self, offset: u64, buf: &[u8]) -> Result<usize> {
            let mut data = self.data.write();
            let off = offset as usize;
            if off + buf.len() > data.len() {
                data.resize(off + buf.len(), 0);
            }
            data[off..off + buf.len()].copy_from_slice(buf);
            Ok(buf.len())
        }
    }

    struct DummyNodeOps;
    impl crate::fs::vfs::NodeOps for DummyNodeOps {}

    #[ktest]
    fn file_backing_size_and_min_size() {
        let dummy_data = Arc::new(RwLock::new(alloc::vec![0u8; 100]));
        let inode = Arc::new(INode::new_with_attr(
            Box::new(DummyNodeOps),
            1,
            INodeAttr {
                size: 100,
                mode: Mode::FILE,
                ..Default::default()
            },
        ));
        let file = Arc::new(File::new(
            Box::new(TestFileOps { data: dummy_data }),
            inode,
            OpenFlags::READ | OpenFlags::WRITE,
            true,
        ));

        let backing1 = FileBacking::new(file.clone()).expect("create backing");
        assert_eq!(backing1.size(), PAGE_SIZE);

        let backing2 = FileBacking::new_with_min_size(file.clone(), 5000).expect("create backing with min size");
        assert_eq!(backing2.size(), 2 * PAGE_SIZE);
    }

    #[ktest]
    fn file_backing_fault_and_flush() {
        let initial = b"Hello PetraOS File Backing!";
        let dummy_data = Arc::new(RwLock::new(Vec::from(&initial[..])));
        let inode = Arc::new(INode::new_with_attr(
            Box::new(DummyNodeOps),
            2,
            INodeAttr {
                size: initial.len(),
                mode: Mode::FILE,
                ..Default::default()
            },
        ));
        let file = Arc::new(File::new(
            Box::new(TestFileOps { data: dummy_data.clone() }),
            inode,
            OpenFlags::READ | OpenFlags::WRITE,
            true,
        ));

        let backing = FileBacking::new(file.clone()).expect("backing");
        let frame = backing.frame(0).expect("frame at 0");

        // Verify read contents
        let mut buf = [0u8; 32];
        let mut reader = frame.reader();
        let mut writer = ostd::mm::VmWriter::from(&mut buf[..]);
        reader.read(&mut writer);
        assert_eq!(&buf[..initial.len()], initial);

        // Modify frame and flush
        let update = b"World PetraOS";
        let mut fwriter = frame.writer();
        let mut ureader = ostd::mm::VmReader::from(&update[..]);
        fwriter.write(&mut ureader);

        backing.flush(0..PAGE_SIZE).expect("flush");

        let cur_data = dummy_data.read();
        assert_eq!(&cur_data[..update.len()], update);
    }
}
