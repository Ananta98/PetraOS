//! Coherent (consistent) DMA buffer allocation.
//!
//! A [`DmaCoherent`] allocates a physically-contiguous run of pages via the
//! buddy allocator and exposes both the device-visible physical address and a
//! kernel virtual address (through the HHDM). The memory is zeroed at
//! allocation time so device-shared structures start in a clean state.
//!
//! On x86-64 the platform maintains DMA cache coherence, so the HHDM mapping
//! (cacheable) is sufficient for long-lived shared structures.

use crate::mm::ensure_mapped;
use crate::mm::hhdm_offset;
use crate::mm::pmm::PMM;
use x86_64::{PhysAddr, VirtAddr};

use super::DmaError;

/// Smallest buddy order (power of two) able to hold `size` bytes.
fn order_for_size(size: usize) -> usize {
    let pages = (size + 4095) / 4096;
    if pages <= 1 {
        return 0;
    }
    let mut order = 0;
    let mut p = 1usize;
    while p < pages {
        p <<= 1;
        order += 1;
    }
    order
}

/// A physically-contiguous, cache-coherent DMA buffer.
///
/// The buffer is freed (returned to the `PMM`) when this value is dropped.
pub struct DmaCoherent {
    phys: PhysAddr,
    virt: VirtAddr,
    size: usize,
    order: usize,
}

impl DmaCoherent {
    /// Allocate a coherent DMA buffer of at least `size` bytes.
    pub fn alloc(size: usize) -> Result<Self, DmaError> {
        if size == 0 {
            return Err(DmaError::InvalidSize);
        }

        let order = order_for_size(size);
        let phys = PMM.alloc_pages(order).ok_or(DmaError::OutOfMemory)?;

        // Guarantee the region is present in the active page table.
        let total = (1usize << order) * 4096;
        ensure_mapped(phys.as_u64(), total);

        let hhdm = hhdm_offset();
        let virt = VirtAddr::new(phys.as_u64() + hhdm);

        // SAFETY: `phys` is a freshly allocated, mapped, exclusive page range.
        unsafe {
            core::ptr::write_bytes(virt.as_mut_ptr::<u8>(), 0, total);
        }

        Ok(Self {
            phys,
            virt,
            size,
            order,
        })
    }

    /// The device-visible physical base address of the buffer.
    pub fn phys(&self) -> PhysAddr {
        self.phys
    }

    /// The kernel virtual base address of the buffer.
    pub fn virt(&self) -> VirtAddr {
        self.virt
    }

    /// Raw immutable pointer to the start of the buffer.
    pub fn as_ptr(&self) -> *const u8 {
        self.virt.as_ptr()
    }

    /// Raw mutable pointer to the start of the buffer.
    pub fn as_mut_ptr(&self) -> *mut u8 {
        self.virt.as_mut_ptr()
    }

    /// Number of bytes requested for the allocation.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Immutable view of the requested portion of the buffer.
    ///
    /// # Safety
    /// The returned slice aliases the raw pointer accessors; treat it like any
    /// other borrow of the underlying memory.
    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: `self.virt` is a valid, mapped, exclusively-owned region.
        unsafe { core::slice::from_raw_parts(self.virt.as_ptr(), self.size) }
    }

    /// Mutable view of the requested portion of the buffer.
    pub fn as_mut_slice(&self) -> &mut [u8] {
        // SAFETY: `self.virt` is a valid, mapped, exclusively-owned region.
        unsafe { core::slice::from_raw_parts_mut(self.virt.as_mut_ptr(), self.size) }
    }
}

impl Drop for DmaCoherent {
    fn drop(&mut self) {
        PMM.free_pages(self.phys, self.order);
    }
}
