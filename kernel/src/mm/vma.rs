use super::address::{PhysAddr, VirtAddr};
use super::paging::{MapError, MapFlags, PageTable, UnmapError};
use alloc::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmAreaKind {
    Anonymous,
    Device { phys_start: PhysAddr },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VmArea {
    pub start: VirtAddr,
    pub end: VirtAddr,
    pub flags: MapFlags,
    pub kind: VmAreaKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddrSpaceError {
    InvalidRange,
    OverlappingArea,
    NoFreeSlots,
    PagingError(MapError),
    UnmapError(UnmapError),
}

pub struct AddrSpace<P: PageTable> {
    page_table: P,
    vm_areas: BTreeMap<VirtAddr, VmArea>,
}

impl<P: PageTable> AddrSpace<P> {
    pub fn new(page_table: P) -> Self {
        Self {
            page_table,
            vm_areas: BTreeMap::new(),
        }
    }

    pub fn page_table(&self) -> &P {
        &self.page_table
    }

    pub fn page_table_mut(&mut self) -> &mut P {
        &mut self.page_table
    }

    /// Map a contiguous range of virtual memory to either physical RAM or device MMIO.
    pub fn map_area(
        &mut self,
        start: VirtAddr,
        size: usize,
        flags: MapFlags,
        kind: VmAreaKind,
    ) -> Result<(), AddrSpaceError> {
        if size == 0 {
            return Err(AddrSpaceError::InvalidRange);
        }
        if !start.is_aligned(4096) || size % 4096 != 0 {
            return Err(AddrSpaceError::InvalidRange);
        }

        let end = start + size;

        // Check for overlapping areas
        for area in self.vm_areas.values() {
            if start < area.end && end > area.start {
                return Err(AddrSpaceError::OverlappingArea);
            }
        }

        // Map pages
        let mut mapped_pages: usize = 0;
        let num_pages = size / 4096;

        for i in 0..num_pages {
            let page_virt = start + (i * 4096);
            let frame_phys = match kind {
                VmAreaKind::Anonymous => {
                    match crate::mm::PMM.alloc_page() {
                        Some(frame) => {
                            // Zero the allocated anonymous physical page
                            let hhdm = super::hhdm_offset();
                            let ptr = frame.as_ptr::<u8>(hhdm);
                            unsafe {
                                core::ptr::write_bytes(ptr, 0, 4096);
                            }
                            frame
                        }
                        None => {
                            // Rollback already mapped pages
                            for j in 0..mapped_pages {
                                let rollback_virt = start + (j * 4096);
                                if let Ok(rollback_frame) = self.page_table.unmap(rollback_virt) {
                                    crate::mm::PMM.free_page(rollback_frame);
                                }
                            }
                            return Err(AddrSpaceError::PagingError(
                                MapError::FrameAllocationFailed,
                            ));
                        }
                    }
                }
                VmAreaKind::Device { phys_start } => phys_start + (i * 4096),
            };

            match self.page_table.map(page_virt, frame_phys, flags) {
                Ok(_) => {
                    mapped_pages += 1;
                }
                Err(err) => {
                    // Rollback mapped pages
                    for j in 0..mapped_pages {
                        let rollback_virt = start + (j * 4096);
                        if let Ok(rollback_frame) = self.page_table.unmap(rollback_virt) {
                            if matches!(kind, VmAreaKind::Anonymous) {
                                crate::mm::PMM.free_page(rollback_frame);
                            }
                        }
                    }
                    return Err(AddrSpaceError::PagingError(err));
                }
            }
        }

        // Store the area
        self.vm_areas.insert(
            start,
            VmArea {
                start,
                end,
                flags,
                kind,
            },
        );

        Ok(())
    }

    /// Unmap a virtual memory area starting at the specified virtual address.
    pub fn unmap_area(&mut self, start: VirtAddr) -> Result<(), AddrSpaceError> {
        let area = self
            .vm_areas
            .remove(&start)
            .ok_or(AddrSpaceError::InvalidRange)?;

        let size = (area.end - area.start) as usize;
        let num_pages = size / 4096;

        for i in 0..num_pages {
            let page_virt = area.start + (i * 4096);
            match self.page_table.unmap(page_virt) {
                Ok(frame) => {
                    if matches!(area.kind, VmAreaKind::Anonymous) {
                        crate::mm::PMM.free_page(frame);
                    }
                }
                Err(err) => {
                    // Restore VMA structure on failure
                    self.vm_areas.insert(start, area);
                    return Err(AddrSpaceError::UnmapError(err));
                }
            }
        }

        Ok(())
    }

    /// Load the associated page table into the CPU's control register.
    ///
    /// # Safety
    /// Caller must guarantee that switching the address space is valid and does not cause kernel page faults.
    pub unsafe fn activate(&self) {
        unsafe {
            self.page_table.activate();
        }
    }
}
