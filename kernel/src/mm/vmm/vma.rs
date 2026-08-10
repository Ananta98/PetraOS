use crate::mm::types::{PhysAddr, VirtAddr};
use crate::mm::vmm::paging::{
    MapError, MapFlags, PageFaultAccess, PageFaultError, PageTable, UnmapError,
};
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

    /// Duplicate the virtual address space using Copy-On-Write (COW) semantics for writable pages.
    pub fn clone(&mut self) -> Result<Self, AddrSpaceError> {
        let mut new_page_table = P::new().map_err(AddrSpaceError::PagingError)?;
        let mut allocated_frames = alloc::vec::Vec::new();

        for (&_vaddr, area) in &self.vm_areas {
            let size = (area.end - area.start) as usize;
            let num_pages = size / 4096;

            for i in 0..num_pages {
                let page_virt = area.start + (i * 4096);
                if let Some(parent_phys) = self.page_table.translate(page_virt) {
                    match area.kind {
                        VmAreaKind::Anonymous => {
                            if area.flags.contains(MapFlags::WRITE) {
                                // COW: Mark both parent and child PTEs as Read-Only + COW
                                let cow_flags = (area.flags & !MapFlags::WRITE) | MapFlags::COW;

                                if let Err(err) = self.page_table.remap(page_virt, cow_flags) {
                                    for frame in allocated_frames {
                                        crate::mm::PMM.free_page(frame);
                                    }
                                    return Err(AddrSpaceError::PagingError(err));
                                }

                                if let Err(err) =
                                    new_page_table.map(page_virt, parent_phys, cow_flags)
                                {
                                    for frame in allocated_frames {
                                        crate::mm::PMM.free_page(frame);
                                    }
                                    return Err(AddrSpaceError::PagingError(err));
                                }

                                crate::mm::PMM.inc_ref(parent_phys);
                            } else {
                                // Read-only anonymous page: share directly
                                if let Err(err) =
                                    new_page_table.map(page_virt, parent_phys, area.flags)
                                {
                                    for frame in allocated_frames {
                                        crate::mm::PMM.free_page(frame);
                                    }
                                    return Err(AddrSpaceError::PagingError(err));
                                }
                                crate::mm::PMM.inc_ref(parent_phys);
                            }
                        }
                        VmAreaKind::Device { .. } => {
                            if let Err(err) = new_page_table.map(page_virt, parent_phys, area.flags)
                            {
                                for frame in allocated_frames {
                                    crate::mm::PMM.free_page(frame);
                                }
                                return Err(AddrSpaceError::PagingError(err));
                            }
                        }
                    }
                }
            }
        }

        Ok(Self {
            page_table: new_page_table,
            vm_areas: self.vm_areas.clone(),
        })
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

        // Eagerly map pages only for Device memory.
        // Anonymous memory is lazily mapped on demand by the page fault handler.
        if matches!(kind, VmAreaKind::Device { .. }) {
            let num_pages = size / 4096;
            let mut mapped_pages: usize = 0;

            for i in 0..num_pages {
                let page_virt = start + (i * 4096);
                let frame_phys = match kind {
                    VmAreaKind::Device { phys_start } => phys_start + (i * 4096),
                    _ => unreachable!(),
                };

                match self.page_table.map(page_virt, frame_phys, flags) {
                    Ok(_) => mapped_pages += 1,
                    Err(err) => {
                        for j in 0..mapped_pages {
                            let rollback_virt = start + (j * 4096);
                            let _ = self.page_table.unmap(rollback_virt);
                        }
                        return Err(AddrSpaceError::PagingError(err));
                    }
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
                Err(UnmapError::NotMapped) => {
                    // Ignore, it was lazily mapped and never allocated.
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

        // 3. Check if page is already mapped in page table
        if let Some((parent_phys, entry_flags)) = self.page_table.get_entry(page_virt) {
            let is_cow_entry = (entry_flags & crate::arch::paging::flags::PAGE_COW) != 0;
            if access.contains(PageFaultAccess::WRITE)
                && (is_cow_entry || area.flags.contains(MapFlags::WRITE))
            {
                let ref_count = crate::mm::PMM.get_ref(parent_phys);
                if ref_count > 1 {
                    // Shared COW frame: allocate a new physical frame and copy contents
                    let new_frame = crate::mm::PMM
                        .alloc_page()
                        .ok_or(PageFaultError::FrameAllocationFailed)?;

                    let hhdm = crate::mm::hhdm_offset();
                    unsafe {
                        let src = parent_phys.as_ptr::<u8>(hhdm);
                        let dest = new_frame.as_ptr::<u8>(hhdm);
                        core::ptr::copy_nonoverlapping(src, dest, 4096);
                    }

                    // Unmap old frame and map new frame with original VMA flags (writable, no COW)
                    let _ = self.page_table.unmap(page_virt);
                    self.page_table
                        .map(page_virt, new_frame, area.flags)
                        .map_err(PageFaultError::PagingError)?;

                    // Decrement reference count on old parent frame
                    crate::mm::PMM.dec_ref(parent_phys);
                } else {
                    // Sole reference remaining: upgrade page flags to Writable (clearing COW)
                    self.page_table
                        .remap(page_virt, area.flags)
                        .map_err(PageFaultError::PagingError)?;
                }
                return Ok(());
            }

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
                let hhdm = crate::mm::hhdm_offset();
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
