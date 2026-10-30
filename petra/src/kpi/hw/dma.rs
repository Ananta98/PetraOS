// SPDX-License-Identifier: GPL-2.0

//! Linux Direct Memory Access (DMA) mapping subsystem (`<linux/dma-mapping.h>`).
//!
//! Provides safe coherent DMA allocations backed by OSTD's [`DmaCoherent`].

use ostd::mm::{
    HasPaddr, PAGE_SIZE, VmIoOnce, VmReader, VmWriter, dma::DmaCoherent,
    io::util::HasVmReaderWriter,
};

use crate::uapi::errno::{Errno, Result};

#[allow(non_camel_case_types)]
pub type dma_addr_t = u64;

/// Safe coherent DMA buffer wrapping OSTD's [`DmaCoherent`].
pub struct DmaBuffer {
    dma: DmaCoherent,
    size: usize,
    paddr: u64,
}

impl DmaBuffer {
    /// Allocates a coherent DMA buffer of at least `size` bytes.
    pub fn alloc(size: usize, zero_filled: bool) -> Result<Self> {
        if size == 0 {
            crate::return_errno!(EINVAL, "zero size DMA buffer allocation");
        }

        let frames = size
            .checked_add(PAGE_SIZE - 1)
            .ok_or(Errno::EINVAL)?
            / PAGE_SIZE;
        let frames = frames.max(1);

        let dma = DmaCoherent::alloc(frames, zero_filled).map_err(|_| Errno::ENOMEM)?;
        let paddr = dma.paddr() as u64;

        Ok(Self { dma, size, paddr })
    }

    /// Returns the physical bus address suitable for hardware DMA programming (`dma_addr_t`).
    #[inline]
    pub fn dma_handle(&self) -> dma_addr_t {
        self.paddr
    }

    /// Returns the requested size in bytes.
    #[inline]
    pub fn size(&self) -> usize {
        self.size
    }

    /// Reads `buf.len()` bytes from the DMA buffer at byte `offset`.
    pub fn read(&self, offset: usize, buf: &mut [u8]) -> Result<()> {
        let end = offset.checked_add(buf.len()).ok_or(Errno::EINVAL)?;
        if end > self.size {
            crate::return_errno!(EINVAL, "DMA buffer read out of bounds");
        }
        let mut reader = self.dma.reader();
        let mut writer = VmWriter::from(&mut buf[..]);
        reader.read(&mut writer);
        Ok(())
    }

    /// Writes `buf.len()` bytes into the DMA buffer at byte `offset`.
    pub fn write(&self, offset: usize, buf: &[u8]) -> Result<()> {
        let end = offset.checked_add(buf.len()).ok_or(Errno::EINVAL)?;
        if end > self.size {
            crate::return_errno!(EINVAL, "DMA buffer write out of bounds");
        }
        let mut writer = self.dma.writer();
        let mut reader = VmReader::from(buf);
        writer.write(&mut reader);
        Ok(())
    }

    /// Reads a POD value of type `T` from `offset`.
    pub fn read_val<T: Copy + 'static + ostd::mm::PodOnce>(&self, offset: usize) -> Result<T> {
        if offset + core::mem::size_of::<T>() > self.size {
            crate::return_errno!(EINVAL, "DMA buffer value read out of bounds");
        }
        self.dma.read_once(offset).map_err(|_| Errno::EIO)
    }

    /// Writes a POD value of type `T` at `offset`.
    pub fn write_val<T: Copy + 'static + ostd::mm::PodOnce>(&self, offset: usize, val: &T) -> Result<()> {
        if offset + core::mem::size_of::<T>() > self.size {
            crate::return_errno!(EINVAL, "DMA buffer value write out of bounds");
        }
        self.dma.write_once(offset, val).map_err(|_| Errno::EIO)
    }

    /// Reference to underlying raw `DmaCoherent`.
    #[inline]
    pub fn inner(&self) -> &DmaCoherent {
        &self.dma
    }
}

/// Linux `dma_alloc_coherent`: allocates a zero-initialized coherent DMA buffer.
pub fn dma_alloc_coherent(size: usize) -> Result<DmaBuffer> {
    DmaBuffer::alloc(size, true)
}

/// Linux `dma_free_coherent`: releases a coherent DMA buffer.
pub fn dma_free_coherent(_buf: DmaBuffer) {
    // Dropping DmaBuffer automatically releases underlying DmaCoherent frames
}
