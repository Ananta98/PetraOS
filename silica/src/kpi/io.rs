// SPDX-License-Identifier: GPL-2.0

//! Linux-shaped MMIO remapping and register accessors.
//!
//! Mirrors Linux `ioremap()` and `ioread*`/`iowrite*` accessors on top of OSTD's
//! safe [`ostd::io::IoMem`] and memory subsystem.
//!
//! # Safety & Soundness
//! Accesses are bounded by the mapped region length and executed using volatile operations
//! or atomic memory ordering, preventing compiler reordering of memory-mapped I/O.

use alloc::sync::Arc;
use ostd::{io::IoMem, mm::VmIoOnce};

use crate::uapi::errno::{Errno, Result};

/// A memory-mapped I/O region, mirroring the return of Linux `ioremap()`.
pub struct IoRegion {
    io_mem: IoMem,
    phys_addr: usize,
    size: usize,
}

impl IoRegion {
    /// Remaps a physical address range into kernel virtual space, mirroring `ioremap(offset, size)`.
    pub fn ioremap(phys_addr: usize, size: usize) -> Result<Self> {
        if size == 0 {
            crate::return_errno!(EINVAL, "zero size ioremap");
        }

        let end = phys_addr.checked_add(size).ok_or(Errno::EINVAL)?;
        let io_mem = IoMem::acquire(phys_addr..end).map_err(|_| Errno::ENOMEM)?;

        Ok(Self {
            io_mem,
            phys_addr,
            size,
        })
    }

    /// Returns the physical start address.
    pub fn phys_addr(&self) -> usize {
        self.phys_addr
    }

    /// Returns the size in bytes of the mapped region.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Reads an 8-bit value at byte offset `offset` (`ioread8`).
    pub fn read8(&self, offset: usize) -> Result<u8> {
        if offset + 1 > self.size {
            crate::return_errno!(EINVAL, "ioread8 offset out of bounds");
        }
        let val: u8 = self.io_mem.read_once(offset).map_err(|_| Errno::EIO)?;
        Ok(val)
    }

    /// Reads a 16-bit little-endian value at byte offset `offset` (`ioread16`).
    pub fn read16(&self, offset: usize) -> Result<u16> {
        if offset + 2 > self.size {
            crate::return_errno!(EINVAL, "ioread16 offset out of bounds");
        }
        let val: u16 = self.io_mem.read_once(offset).map_err(|_| Errno::EIO)?;
        Ok(val)
    }

    /// Reads a 32-bit little-endian value at byte offset `offset` (`ioread32`).
    pub fn read32(&self, offset: usize) -> Result<u32> {
        if offset + 4 > self.size {
            crate::return_errno!(EINVAL, "ioread32 offset out of bounds");
        }
        let val: u32 = self.io_mem.read_once(offset).map_err(|_| Errno::EIO)?;
        Ok(val)
    }

    /// Reads a 64-bit little-endian value at byte offset `offset` (`ioread64`).
    pub fn read64(&self, offset: usize) -> Result<u64> {
        if offset + 8 > self.size {
            crate::return_errno!(EINVAL, "ioread64 offset out of bounds");
        }
        let val: u64 = self.io_mem.read_once(offset).map_err(|_| Errno::EIO)?;
        Ok(val)
    }

    /// Writes an 8-bit value at byte offset `offset` (`iowrite8`).
    pub fn write8(&self, offset: usize, val: u8) -> Result<()> {
        if offset + 1 > self.size {
            crate::return_errno!(EINVAL, "iowrite8 offset out of bounds");
        }
        self.io_mem
            .write_once(offset, &val)
            .map_err(|_| Errno::EIO)?;
        Ok(())
    }

    /// Writes a 16-bit little-endian value at byte offset `offset` (`iowrite16`).
    pub fn write16(&self, offset: usize, val: u16) -> Result<()> {
        if offset + 2 > self.size {
            crate::return_errno!(EINVAL, "iowrite16 offset out of bounds");
        }
        self.io_mem
            .write_once(offset, &val)
            .map_err(|_| Errno::EIO)?;
        Ok(())
    }

    /// Writes a 32-bit little-endian value at byte offset `offset` (`iowrite32`).
    pub fn write32(&self, offset: usize, val: u32) -> Result<()> {
        if offset + 4 > self.size {
            crate::return_errno!(EINVAL, "iowrite32 offset out of bounds");
        }
        self.io_mem
            .write_once(offset, &val)
            .map_err(|_| Errno::EIO)?;
        Ok(())
    }

    /// Writes a 64-bit little-endian value at byte offset `offset` (`iowrite64`).
    pub fn write64(&self, offset: usize, val: u64) -> Result<()> {
        if offset + 8 > self.size {
            crate::return_errno!(EINVAL, "iowrite64 offset out of bounds");
        }
        self.io_mem
            .write_once(offset, &val)
            .map_err(|_| Errno::EIO)?;
        Ok(())
    }

    /// Reference to the underlying raw `IoMem` structure.
    pub fn io_mem(&self) -> &IoMem {
        &self.io_mem
    }
}

/// Linux `ioremap`: maps physical MMIO into kernel virtual memory space.
pub fn ioremap(phys_addr: usize, size: usize) -> Result<IoRegion> {
    IoRegion::ioremap(phys_addr, size)
}

/// Linux `ioremap_shared`: maps physical MMIO into an atomically shared handle `Arc<IoRegion>`.
pub fn ioremap_shared(phys_addr: usize, size: usize) -> Result<Arc<IoRegion>> {
    Ok(Arc::new(ioremap(phys_addr, size)?))
}
