use super::address::{PhysAddr, VirtAddr};
use super::paging::{MapError, MapFlags, PageFaultAccess, PageFaultError, PageTable, UnmapError};
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

impl VmArea {
    /// Returns true if this VMA contains the specified virtual address.
    #[inline]
    pub fn contains(&self, addr: VirtAddr) -> bool {
        addr >= self.start && addr < self.end
    }

    /// Returns true if two VMAs have identical permissions and backing kind and can be merged.
    #[inline]
    pub fn can_merge(&self, other: &Self) -> bool {
        self.flags == other.flags && self.kind == other.kind
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddrSpaceError {
    InvalidRange,
    OverlappingArea,
    NoFreeSlots,
    PagingError(MapError),
    UnmapError(UnmapError),
}

/// Architecture-independent Virtual Memory Address Space representation.
///
/// Wraps a hardware page table implementation `P` and manages registered `VmArea` regions.
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

    /// Efficiently lookup the VMA containing the specified virtual address ($O(\log N)$).
    pub fn find_vma(&self, addr: VirtAddr) -> Option<&VmArea> {
        self.vm_areas
            .range(..=addr)
            .next_back()
            .map(|(_, area)| area)
            .filter(|area| area.contains(addr))
    }

    /// Efficiently lookup the mutable VMA containing the specified virtual address ($O(\log N)$).
    pub fn find_vma_mut(&mut self, addr: VirtAddr) -> Option<&mut VmArea> {
        self.vm_areas
            .range_mut(..=addr)
            .next_back()
            .map(|(_, area)| area)
            .filter(|area| area.contains(addr))
    }

    /// Check if virtual address range `[start, end)` overlaps with any existing VMA ($O(\log N)$).
    fn check_overlap(&self, start: VirtAddr, end: VirtAddr) -> bool {
        if let Some((_, area)) = self.vm_areas.range(..end).next_back() {
            start < area.end && end > area.start
        } else {
            false
        }
    }

    /// Register a virtual memory area lazily without immediately mapping physical pages ($O(\log N)$).
    /// Physical pages will be populated on demand via `handle_page_fault`.
    pub fn map_area_lazy(
        &mut self,
        start: VirtAddr,
        size: usize,
        flags: MapFlags,
        kind: VmAreaKind,
    ) -> Result<(), AddrSpaceError> {
        if size == 0 || !start.is_aligned(4096) || size % 4096 != 0 {
            return Err(AddrSpaceError::InvalidRange);
        }

        let mut end = start + size;

        // Optimized O(log N) overlap check
        if self.check_overlap(start, end) {
            return Err(AddrSpaceError::OverlappingArea);
        }

        let mut final_start = start;

        // Attempt VMA Coalescing with predecessor
        if let Some((&pred_start, pred_area)) = self.vm_areas.range(..start).next_back() {
            if pred_area.end == start && pred_area.flags == flags && pred_area.kind == kind {
                final_start = pred_start;
                end = pred_area.end.max(end);
                self.vm_areas.remove(&pred_start);
            }
        }

        // Attempt VMA Coalescing with successor
        if let Some((&succ_start, succ_area)) = self.vm_areas.range(start..).next() {
            if succ_start == end && succ_area.flags == flags && succ_area.kind == kind {
                end = succ_area.end;
                self.vm_areas.remove(&succ_start);
            }
        }

        self.vm_areas.insert(
            final_start,
            VmArea {
                start: final_start,
                end,
                flags,
                kind,
            },
        );

        Ok(())
    }

    /// Map a contiguous range of virtual memory to physical RAM or MMIO immediately (eager mapping).
    pub fn map_area(
        &mut self,
        start: VirtAddr,
        size: usize,
        flags: MapFlags,
        kind: VmAreaKind,
    ) -> Result<(), AddrSpaceError> {
        if size == 0 || !start.is_aligned(4096) || size % 4096 != 0 {
            return Err(AddrSpaceError::InvalidRange);
        }

        let end = start + size;

        // Optimized O(log N) overlap check
        if self.check_overlap(start, end) {
            return Err(AddrSpaceError::OverlappingArea);
        }

        // Eagerly map pages into page table
        let num_pages = size / 4096;
        let mut mapped_pages: usize = 0;

        for i in 0..num_pages {
            let page_virt = start + (i * 4096);
            let frame_phys = match kind {
                VmAreaKind::Anonymous => match crate::mm::PMM.alloc_page() {
                    Some(frame) => {
                        let hhdm = super::hhdm_offset();
                        let ptr = frame.as_ptr::<u8>(hhdm);
                        unsafe {
                            core::ptr::write_bytes(ptr, 0, 4096);
                        }
                        frame
                    }
                    None => {
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
                },
                VmAreaKind::Device { phys_start } => phys_start + (i * 4096),
            };

            match self.page_table.map(page_virt, frame_phys, flags) {
                Ok(_) => {
                    mapped_pages += 1;
                }
                Err(err) => {
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

    /// Architecture-independent Page Fault Resolution Algorithm.
    ///
    /// Evaluates virtual address fault against registered VMAs, checks access permissions,
    /// and resolves demand paging / lazy allocation.
    pub fn handle_page_fault(
        &mut self,
        fault_addr: VirtAddr,
        access: PageFaultAccess,
    ) -> Result<(), PageFaultError> {
        // 1. Locate VMA covering fault_addr in O(log N)
        let area = match self.find_vma(fault_addr) {
            Some(vma) => *vma,
            None => return Err(PageFaultError::UnmappedAccess),
        };

        // 2. Validate access permissions
        if access.contains(PageFaultAccess::WRITE) && !area.flags.contains(MapFlags::WRITE) {
            return Err(PageFaultError::ProtectionViolation);
        }
        if access.contains(PageFaultAccess::EXECUTE) && !area.flags.contains(MapFlags::EXECUTE) {
            return Err(PageFaultError::ProtectionViolation);
        }
        if access.contains(PageFaultAccess::USER) && !area.flags.contains(MapFlags::USER) {
            return Err(PageFaultError::ProtectionViolation);
        }

        let page_virt = VirtAddr(fault_addr.as_u64() & !4095);

        // 3. Check if page is already mapped
        if self.page_table.translate(page_virt).is_some() {
            if access.contains(PageFaultAccess::PRESENT) {
                return Err(PageFaultError::ProtectionViolation);
            }
            return Ok(()); // Spurious fault
        }

        // 4. Resolve Demand Paging / Allocation
        let frame_phys = match area.kind {
            VmAreaKind::Anonymous => {
                let frame = crate::mm::PMM
                    .alloc_page()
                    .ok_or(PageFaultError::FrameAllocationFailed)?;
                let hhdm = super::hhdm_offset();
                let ptr = frame.as_ptr::<u8>(hhdm);
                unsafe {
                    core::ptr::write_bytes(ptr, 0, 4096);
                }
                frame
            }
            VmAreaKind::Device { phys_start } => {
                let offset = (page_virt - area.start) as u64;
                phys_start + offset
            }
        };

        // 5. Map virtual page into CPU page table
        self.page_table
            .map(page_virt, frame_phys, area.flags)
            .map_err(PageFaultError::PagingError)?;

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
