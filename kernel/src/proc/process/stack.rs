//! Kernel Execution Stack for Process/Thread Management.
//!
//! Provides a page-allocated kernel stack used during context switches and
//! system calls. Ownership lives in the process subsystem as stacks are
//! a process-level resource, not an architecture primitive.

use crate::mm::{PMM, PhysAddr, VirtAddr, hhdm_offset};

/// A page-allocated kernel stack with virtual base and top addresses.
pub struct KernelStack {
    base: VirtAddr,
    top: VirtAddr,
}

impl KernelStack {
    /// Usable kernel stack size (16 KiB = 4 pages, order 2).
    pub const STACK_SIZE: usize = 16 * 1024;
    pub const STACK_ORDER: usize = 2;

    /// Allocate a new page-backed kernel stack from the Physical Memory Manager (PMM).
    pub fn new() -> Result<Self, &'static str> {
        let phys = PMM
            .alloc_pages(Self::STACK_ORDER)
            .ok_or("Failed to allocate physical pages for kernel stack")?;
        let hhdm = hhdm_offset();
        let base = VirtAddr::new(phys.as_u64() + hhdm);
        let top = VirtAddr::new(base.as_u64() + Self::STACK_SIZE as u64);

        // SAFETY: The allocated physical frame is valid and mapped in HHDM.
        unsafe {
            core::ptr::write_bytes(base.as_mut_ptr::<u8>(), 0, Self::STACK_SIZE);
        }

        Ok(Self { base, top })
    }

    /// Returns the 16-byte aligned top virtual address of the stack.
    #[inline(always)]
    pub fn top(&self) -> VirtAddr {
        VirtAddr::new(self.top.as_u64() & !15)
    }

    /// Returns the total guarded/allocated stack size in bytes.
    #[inline(always)]
    pub const fn guarded_size() -> usize {
        Self::STACK_SIZE
    }
}

impl Drop for KernelStack {
    fn drop(&mut self) {
        if !self.base.is_null() {
            let hhdm = hhdm_offset();
            let phys = PhysAddr::new(self.base.as_u64() - hhdm);
            PMM.free_pages(phys, Self::STACK_ORDER);
        }
    }
}
