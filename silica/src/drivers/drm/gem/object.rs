// SPDX-License-Identifier: GPL-2.0

//! GEM buffer objects ([`GemObject`]).

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use core::ops::Range;
use core::sync::atomic::{AtomicU32, Ordering};
use ostd::{
    mm::{
        HasPaddr, PAGE_SIZE, UFrame, VmReader, VmWriter, dma::DmaCoherent,
        io::util::HasVmReaderWriter,
    },
    sync::SpinLock,
};

use crate::api::errno::{Errno, Result};
use crate::vm::backing::alloc_zeroed_frame;

/// Next object instance counter; objects get distinguishing allocation ids.
static NEXT_ID: AtomicU32 = AtomicU32::new(1);

/// A GEM buffer object.
///
/// A GEM object owns a fixed-size, physically-contiguous, cache-coherent DMA
/// region. It is the kernel-side unit of video memory that user space
/// allocates, maps, attaches to scanout hardware and hands to the GPU drivers
/// of [`crate::drivers::drm`].
///
/// One physical allocation exists per object; the same object may be
/// referenced by many per-file handles and mappings through additional
/// `Arc<GemObject>` clones.
pub struct GemObject {
    /// Page-aligned size of the object in bytes.
    size: usize,
    /// The DMA-coherent backing store, zeroed at allocation time.
    dma: DmaCoherent,
    /// Resident user-mapping frames keyed by page offset in the object.
    ///
    /// The cache is shared by every mapping of the object, so two `mmap(2)`s
    /// of one object see the same frames; `flush_pages` copies it back into
    /// the DMA region, which is what scanout hardware consumes.
    pages: SpinLock<BTreeMap<usize, UFrame>>,
    /// Monotonic allocation-order identity of the object.
    pub id: u32,
}

impl GemObject {
    /// Allocates a zeroed GEM object of `size` bytes, rounded up to a whole
    /// number of pages.
    pub fn alloc(size: usize) -> Result<Arc<Self>> {
        if size == 0 {
            crate::return_errno!(EINVAL, "a GEM object cannot be empty");
        }
        let aligned = size.div_ceil(PAGE_SIZE) * PAGE_SIZE;
        let sz = aligned / PAGE_SIZE;
        let dma = DmaCoherent::alloc(sz, true).map_err(|_| Errno::ENOMEM)?;
        Ok(Arc::new(Self {
            size: aligned,
            dma,
            pages: SpinLock::new(BTreeMap::new()),
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        }))
    }

    /// Returns the page-aligned size of the object in bytes.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Returns the physical address of the object's first byte for
    /// scanout/DMA programming. CPU access goes through the DMA reader/writer
    /// or through a user-space mapping instead.
    pub fn paddr(&self) -> usize {
        self.dma.paddr()
    }

    /// The object's DMA storage, for drivers that program device MMIO
    /// directly from it.
    pub(crate) fn dma(&self) -> &DmaCoherent {
        &self.dma
    }

    /// Writes `buf` into the object's DMA region at `offset`.
    pub fn write_at(&self, offset: usize, buf: &[u8]) -> Result<()> {
        if offset.checked_add(buf.len()).ok_or(Errno::EINVAL)? > self.size {
            crate::return_errno!(EINVAL, "writing past the end of the buffer");
        }
        let mut writer = self.dma.writer();
        writer.skip(offset);
        writer.write(&mut VmReader::from(buf));
        Ok(())
    }

    /// Copies the object's DMA region at `offset` into `buf`.
    pub fn read_at(&self, offset: usize, buf: &mut [u8]) -> Result<()> {
        if offset.checked_add(buf.len()).ok_or(Errno::EINVAL)? > self.size {
            crate::return_errno!(EINVAL, "reading past the end of the buffer");
        }
        let mut reader = self.dma.reader();
        reader.skip(offset);
        reader.read(&mut VmWriter::from(buf));
        Ok(())
    }

    /// Resident mapping page for a page offset in the object, newly copied
    /// from the DMA region (which is authoritative after boot) if missing.
    pub(crate) fn cached_frame(&self, page: usize) -> Result<UFrame> {
        if page % PAGE_SIZE != 0 || page >= self.size {
            crate::return_errno!(EINVAL, "page offset out of the object");
        }

        let mut pages = self.pages.lock();
        if let Some(frame) = pages.get(&page) {
            return Ok(frame.clone());
        }

        let frame = alloc_zeroed_frame()?;
        let mut reader = self.dma.reader();
        reader.skip(page);
        reader.read(&mut frame.clone().writer());
        pages.insert(page, frame.clone());
        Ok(frame)
    }

    /// Copies every resident mapping page within `range` back into the DMA
    /// region, the writeback half of user-space drawing.
    pub(crate) fn flush_pages(&self, range: Range<usize>) {
        let pages = self.pages.lock();
        for (&page, frame) in pages.range(range) {
            let mut writer = self.dma.writer();
            writer.skip(page);
            writer.write(&mut frame.clone().reader());
        }
    }

    /// Drops resident mapping pages, forcing them to be re-copied from the
    /// DMA region on the next fault. Called on invalidating flushes.
    pub(crate) fn invalidate_pages(&self) {
        self.pages.lock().clear();
    }
}

impl core::fmt::Debug for GemObject {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("GemObject")
            .field("id", &self.id)
            .field("size", &self.size)
            .field("paddr", &(self.dma.paddr() as u64))
            .finish()
    }
}
