// SPDX-License-Identifier: GPL-2.0

//! GEM buffer objects ([`GemObject`]).

use alloc::sync::Arc;
use core::ops::Range;
use core::sync::atomic::{AtomicU32, Ordering};
use ostd::mm::{
    FrameAllocOptions, HasDaddr, HasPaddr, PAGE_SIZE, UFrame, USegment, VmReader, VmWriter,
    dma::{DmaStream, FromAndToDevice},
    io::util::HasVmReaderWriter,
};

use crate::uapi::errno::{Errno, Result};

/// Next object instance counter; objects get distinguishing allocation ids.
static NEXT_ID: AtomicU32 = AtomicU32::new(1);

/// A GEM buffer object.
///
/// A GEM object owns a single, physically-contiguous, zero-copy DMA region
/// (`USegment` mapped via `DmaStream`). User-space `mmap(2)` mappings map the
/// exact same physical frames directly, eliminating duplicate page storage and
/// CPU writeback copies on `DIRTYFB`.
pub struct GemObject {
    /// Page-aligned size of the object in bytes.
    size: usize,
    /// Physical frame segment shared directly with user-space mappings.
    segment: USegment,
    /// Streaming DMA mapping over `segment` for scanout/GPU hardware.
    dma: DmaStream<FromAndToDevice>,
    /// Monotonic allocation-order identity of the object.
    pub id: u32,
}

impl Drop for GemObject {
    fn drop(&mut self) {
        crate::utils::perf::sub_gem_bytes(self.size);
    }
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
        let raw_seg = FrameAllocOptions::new()
            .zeroed(true)
            .alloc_segment(sz)
            .map_err(|_| Errno::ENOMEM)?;
        let segment = USegment::from(raw_seg);
        let dma = DmaStream::map(segment.clone(), true).map_err(|_| Errno::ENOMEM)?;
        crate::utils::perf::add_gem_bytes(aligned);
        Ok(Arc::new(Self {
            size: aligned,
            segment,
            dma,
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        }))
    }

    /// Returns the page-aligned size of the object in bytes.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Returns the physical address of the object's first byte for
    /// scanout/DMA programming.
    pub fn paddr(&self) -> usize {
        self.dma.paddr()
    }

    /// Returns the bus/device DMA address of the object's first byte.
    pub fn daddr(&self) -> usize {
        self.dma.daddr()
    }

    /// The object's DMA storage, for drivers that program device MMIO
    /// directly from it.
    pub(crate) fn dma(&self) -> &DmaStream<FromAndToDevice> {
        &self.dma
    }

    /// Writes `buf` into the object's DMA region at `offset`.
    pub fn write_at(&self, offset: usize, buf: &[u8]) -> Result<()> {
        if offset.checked_add(buf.len()).ok_or(Errno::EINVAL)? > self.size {
            crate::return_errno!(EINVAL, "writing past the end of the buffer");
        }
        let mut writer = self.dma.writer().map_err(|_| Errno::EIO)?;
        writer.skip(offset);
        writer.write(&mut VmReader::from(buf));
        Ok(())
    }

    /// Copies the object's DMA region at `offset` into `buf`.
    pub fn read_at(&self, offset: usize, buf: &mut [u8]) -> Result<()> {
        if offset.checked_add(buf.len()).ok_or(Errno::EINVAL)? > self.size {
            crate::return_errno!(EINVAL, "reading past the end of the buffer");
        }
        let mut reader = self.dma.reader().map_err(|_| Errno::EIO)?;
        reader.skip(offset);
        reader.read(&mut VmWriter::from(buf));
        Ok(())
    }

    /// Returns the zero-copy `UFrame` at `page` directly from the underlying DMA segment.
    pub(crate) fn cached_frame(&self, page: usize) -> Result<UFrame> {
        if page % PAGE_SIZE != 0 || page >= self.size {
            crate::return_errno!(EINVAL, "page offset out of the object");
        }
        self.segment
            .slice(&(page..page + PAGE_SIZE))
            .next()
            .ok_or(Errno::EINVAL)
    }

    /// Synchronizes `range` to the device for DMA (no CPU memcpy needed).
    pub(crate) fn flush_pages(&self, range: Range<usize>) {
        let start = range.start.min(self.size);
        let end = range.end.min(self.size);
        if start < end {
            let _ = self.dma.sync_to_device(start..end);
        }
    }

    /// Synchronizes the entire object from the device.
    pub(crate) fn invalidate_pages(&self) {
        let _ = self.dma.sync_from_device(0..self.size);
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
