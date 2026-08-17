use crate::mm::vmm::paging::PageTable;
use crate::mm::vmm::types::VmAreaKind;
use alloc::collections::BTreeMap;
use x86_64::structures::idt::PageFaultErrorCode;
use x86_64::structures::paging::mapper::{FlagUpdateError, MapToError, UnmapError};
use x86_64::structures::paging::{PageTableFlags, Size4KiB};
use x86_64::{PhysAddr, VirtAddr};

pub const COW_FLAG: PageTableFlags = PageTableFlags::BIT_9;

#[derive(Clone, PartialEq, Eq)]
pub struct VmArea {
    pub start: VirtAddr,
    pub end: VirtAddr,
    pub flags: PageTableFlags,
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

#[derive(Debug)]
pub enum AddrSpaceError {
    InvalidRange,
    OverlappingArea,
    NoFreeSlots,
    PagingError(MapToError<Size4KiB>),
    UnmapError(UnmapError),
    FlagUpdateError(FlagUpdateError),
}

#[derive(Debug)]
pub enum PageFaultError {
    UnmappedAccess,        // Virtual address is not within any registered VMA
    ProtectionViolation,   // VMA flags disallow the requested access mode
    FrameAllocationFailed, // Physical memory allocator ran out of pages
    PagingError(MapToError<Size4KiB>), // Failure while updating page tables
    RemapError(FlagUpdateError),
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

    /// Duplicate the virtual address space using Copy-On-Write (COW) semantics for writable pages.
    pub fn clone(&mut self) -> Result<Self, AddrSpaceError> {
        let mut new_page_table = P::new().map_err(AddrSpaceError::PagingError)?;

        // Track every child mapping we've made: (virt, phys, was_cow).
        // On failure we use this to unmap + dec_ref + revert parent PTEs.
        let mut child_maps: alloc::vec::Vec<(VirtAddr, PhysAddr, bool)> = alloc::vec::Vec::new();

        for (&_vaddr, area) in &self.vm_areas {
            let size = (area.end - area.start) as usize;
            let num_pages = size / 4096;

            'pages: for i in 0..num_pages {
                let page_virt = area.start + (i as u64 * 4096);
                let parent_phys = match self.page_table.translate(page_virt) {
                    Some(p) => p,
                    None => continue 'pages,
                };

                match &area.kind {
                    VmAreaKind::Anonymous | VmAreaKind::File { .. } => {
                        if area.flags.contains(PageTableFlags::WRITABLE) {
                            // COW: mark parent PTE read-only + COW first.
                            let cow_flags = (area.flags & !PageTableFlags::WRITABLE) | COW_FLAG;
                            if let Err(err) = self.page_table.remap(page_virt, cow_flags) {
                                Self::rollback_clone(
                                    &mut self.page_table,
                                    &mut new_page_table,
                                    &child_maps,
                                    &self.vm_areas,
                                );
                                return Err(AddrSpaceError::FlagUpdateError(err));
                            }
                            // Map the same frame into the child under COW flags.
                            if let Err(err) = new_page_table.map(page_virt, parent_phys, cow_flags) {
                                // Revert the parent remap we just did.
                                let _ = self.page_table.remap(page_virt, area.flags);
                                Self::rollback_clone(
                                    &mut self.page_table,
                                    &mut new_page_table,
                                    &child_maps,
                                    &self.vm_areas,
                                );
                                return Err(AddrSpaceError::PagingError(err));
                            }
                            crate::mm::PMM.inc_ref(parent_phys);
                            child_maps.push((page_virt, parent_phys, true));
                        } else {
                            // Read-only page: share directly without COW remap.
                            if let Err(err) = new_page_table.map(page_virt, parent_phys, area.flags) {
                                Self::rollback_clone(
                                    &mut self.page_table,
                                    &mut new_page_table,
                                    &child_maps,
                                    &self.vm_areas,
                                );
                                return Err(AddrSpaceError::PagingError(err));
                            }
                            crate::mm::PMM.inc_ref(parent_phys);
                            child_maps.push((page_virt, parent_phys, false));
                        }
                    }
                    VmAreaKind::Device { .. } => {
                        // Device pages are shared as-is with no refcount.
                        if let Err(err) = new_page_table.map(page_virt, parent_phys, area.flags) {
                            Self::rollback_clone(
                                &mut self.page_table,
                                &mut new_page_table,
                                &child_maps,
                                &self.vm_areas,
                            );
                            return Err(AddrSpaceError::PagingError(err));
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

    /// Undo a partial clone on error: unmap all child pages, dec_ref their frames,
    /// and revert any parent PTEs that were COW-remapped back to their original flags.
    fn rollback_clone(
        parent_pt: &mut P,
        child_pt: &mut P,
        child_maps: &[(VirtAddr, PhysAddr, bool)],
        vm_areas: &alloc::collections::BTreeMap<VirtAddr, VmArea>,
    ) {
        for &(virt, phys, was_cow) in child_maps {
            let _ = child_pt.unmap(virt);
            crate::mm::PMM.dec_ref(phys);
            if was_cow {
                // Restore parent PTE to its original writable flags.
                if let Some(area) = vm_areas.range(..=virt).next_back().map(|(_, a)| a) {
                    if area.contains(virt) {
                        let _ = parent_pt.remap(virt, area.flags);
                    }
                }
            }
        }
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

    /// Map a contiguous range of virtual memory to physical RAM, MMIO, or a File eagerly.
    pub fn map_area(
        &mut self,
        start: VirtAddr,
        size: usize,
        flags: PageTableFlags,
        kind: VmAreaKind,
    ) -> Result<(), AddrSpaceError> {
        if size == 0 || !start.is_aligned(4096u64) || size % 4096 != 0 {
            return Err(AddrSpaceError::InvalidRange);
        }

        let end = start + size as u64;

        // Optimized O(log N) overlap check
        if self.check_overlap(start, end) {
            return Err(AddrSpaceError::OverlappingArea);
        }

        let num_pages = size / 4096;
        let mut mapped_pages: usize = 0;
        let hhdm = crate::mm::hhdm_offset();

        for i in 0..num_pages {
            let page_virt = start + (i as u64 * 4096);
            let frame_phys = match &kind {
                VmAreaKind::Anonymous => {
                    let frame = match crate::mm::PMM.alloc_page() {
                        Some(f) => f,
                        None => {
                            self.rollback_mapping(start, mapped_pages, &kind);
                            return Err(AddrSpaceError::PagingError(MapToError::FrameAllocationFailed));
                        }
                    };
                    let dest_ptr = (frame.as_u64() + hhdm) as *mut u8;
                    // SAFETY: Zeroing newly allocated anonymous physical frame.
                    unsafe {
                        core::ptr::write_bytes(dest_ptr, 0, 4096);
                    }
                    frame
                }
                VmAreaKind::Device { phys_start } => *phys_start + (i as u64 * 4096),
                VmAreaKind::File {
                    file,
                    offset,
                    file_size,
                } => {
                    let frame = match crate::mm::PMM.alloc_page() {
                        Some(f) => f,
                        None => {
                            self.rollback_mapping(start, mapped_pages, &kind);
                            return Err(AddrSpaceError::PagingError(MapToError::FrameAllocationFailed));
                        }
                    };
                    let dest_ptr = (frame.as_u64() + hhdm) as *mut u8;

                    let page_file_offset = offset + (i * 4096);
                    let bytes_written = if page_file_offset < *file_size {
                        let bytes_to_read = core::cmp::min(4096, *file_size - page_file_offset);
                        let buf_slice =
                            unsafe { core::slice::from_raw_parts_mut(dest_ptr, bytes_to_read) };
                        let _ = file.read(page_file_offset, buf_slice);
                        bytes_to_read
                    } else {
                        0
                    };

                    // SAFETY: Zero only the bytes beyond what the file filled in,
                    // avoiding a full write_bytes when the entire page came from the file.
                    if bytes_written < 4096 {
                        unsafe {
                            core::ptr::write_bytes(dest_ptr.add(bytes_written), 0, 4096 - bytes_written);
                        }
                    }
                    frame
                }
            };

            match self.page_table.map(page_virt, frame_phys, flags) {
                Ok(_) => mapped_pages += 1,
                Err(err) => {
                    self.rollback_mapping(start, mapped_pages, &kind);
                    if matches!(kind, VmAreaKind::Anonymous | VmAreaKind::File { .. }) {
                        crate::mm::PMM.free_page(frame_phys);
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

    fn rollback_mapping(&mut self, start: VirtAddr, mapped_pages: usize, kind: &VmAreaKind) {
        for j in 0..mapped_pages {
            let rollback_virt = start + (j as u64 * 4096);
            if let Ok(frame) = self.page_table.unmap(rollback_virt) {
                if matches!(kind, VmAreaKind::Anonymous | VmAreaKind::File { .. }) {
                    crate::mm::PMM.free_page(frame);
                }
            }
        }
    }

    /// Unmap and remove any VMAs or parts of VMAs overlapping [start, end).
    pub fn unmap_range(&mut self, start: VirtAddr, end: VirtAddr) -> Result<(), AddrSpaceError> {
        let mut to_remove = alloc::vec::Vec::new();
        let mut to_add = alloc::vec::Vec::new();

        for (&vma_start, vma) in &self.vm_areas {
            if vma.start < end && vma.end > start {
                to_remove.push(vma_start);

                if vma.start < start {
                    to_add.push(VmArea {
                        start: vma.start,
                        end: start,
                        flags: vma.flags,
                        kind: vma.kind.clone(),
                    });
                }

                if vma.end > end {
                    to_add.push(VmArea {
                        start: end,
                        end: vma.end,
                        flags: vma.flags,
                        kind: vma.kind.clone(),
                    });
                }
            }
        }

        for k in to_remove {
            self.vm_areas.remove(&k);
        }
        for v in to_add {
            self.vm_areas.insert(v.start, v);
        }

        for page_virt_u64 in (start.as_u64()..end.as_u64()).step_by(4096) {
            let page_virt = VirtAddr::new(page_virt_u64);
            if let Ok(old_frame) = self.page_table.unmap(page_virt) {
                crate::mm::PMM.free_page(old_frame);
            }
        }

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
            let page_virt = area.start + (i as u64 * 4096);
            match self.page_table.unmap(page_virt) {
                Ok(frame) => {
                    if matches!(area.kind, VmAreaKind::Anonymous | VmAreaKind::File { .. }) {
                        crate::mm::PMM.free_page(frame);
                    }
                }
                Err(UnmapError::PageNotMapped) => {}
                Err(err) => {
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
    /// and resolves Copy-On-Write (COW).
    pub fn handle_page_fault(
        &mut self,
        fault_addr: VirtAddr,
        access: PageFaultErrorCode,
    ) -> Result<(), PageFaultError> {
        // 1. Locate VMA covering fault_addr in O(log N)
        let area = match self.find_vma(fault_addr) {
            Some(vma) => vma.clone(),
            None => return Err(PageFaultError::UnmappedAccess),
        };

        // 2. Validate access permissions
        if access.contains(PageFaultErrorCode::CAUSED_BY_WRITE)
            && !area.flags.contains(PageTableFlags::WRITABLE)
        {
            return Err(PageFaultError::ProtectionViolation);
        }
        if access.contains(PageFaultErrorCode::INSTRUCTION_FETCH)
            && area.flags.contains(PageTableFlags::NO_EXECUTE)
        {
            return Err(PageFaultError::ProtectionViolation);
        }
        if access.contains(PageFaultErrorCode::USER_MODE)
            && !area.flags.contains(PageTableFlags::USER_ACCESSIBLE)
        {
            return Err(PageFaultError::ProtectionViolation);
        }

        let page_virt = VirtAddr::new(fault_addr.as_u64() & !4095);

        // 3. Check if page is present in page table for COW resolution
        if let Some((parent_phys, entry_flags)) = self.page_table.get_entry(page_virt) {
            let is_cow_entry = entry_flags.contains(COW_FLAG);
            if access.contains(PageFaultErrorCode::CAUSED_BY_WRITE)
                && (is_cow_entry || area.flags.contains(PageTableFlags::WRITABLE))
            {
                let ref_count = crate::mm::PMM.get_ref(parent_phys);
                if ref_count > 1 {
                    // Shared COW frame: allocate a new physical frame and copy contents
                    let new_frame = crate::mm::PMM
                        .alloc_page()
                        .ok_or(PageFaultError::FrameAllocationFailed)?;

                    let hhdm = crate::mm::hhdm_offset();
                    unsafe {
                        let src = (parent_phys.as_u64() + hhdm) as *const u8;
                        let dest = (new_frame.as_u64() + hhdm) as *mut u8;
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
                        .map_err(PageFaultError::RemapError)?;
                }
                return Ok(());
            }

            if access.contains(PageFaultErrorCode::PROTECTION_VIOLATION) {
                return Err(PageFaultError::ProtectionViolation);
            }
            return Ok(()); // Spurious fault
        }

        // 4. Page is not present in hardware page table: handle demand paging for registered VMA
        let hhdm = crate::mm::hhdm_offset();
        let frame_phys = match &area.kind {
            VmAreaKind::Anonymous => {
                let frame = crate::mm::PMM
                    .alloc_page()
                    .ok_or(PageFaultError::FrameAllocationFailed)?;
                let dest_ptr = (frame.as_u64() + hhdm) as *mut u8;
                // SAFETY: Zeroing newly allocated anonymous physical frame.
                unsafe {
                    core::ptr::write_bytes(dest_ptr, 0, 4096);
                }
                frame
            }
            VmAreaKind::Device { phys_start } => {
                let page_offset = page_virt - area.start;
                *phys_start + page_offset
            }
            VmAreaKind::File {
                file,
                offset,
                file_size,
            } => {
                let frame = crate::mm::PMM
                    .alloc_page()
                    .ok_or(PageFaultError::FrameAllocationFailed)?;
                let dest_ptr = (frame.as_u64() + hhdm) as *mut u8;

                let page_file_offset = offset + (page_virt - area.start) as usize;
                let bytes_written = if page_file_offset < *file_size {
                    let bytes_to_read = core::cmp::min(4096, *file_size - page_file_offset);
                    let buf_slice =
                        unsafe { core::slice::from_raw_parts_mut(dest_ptr, bytes_to_read) };
                    let _ = file.read(page_file_offset, buf_slice);
                    bytes_to_read
                } else {
                    0
                };

                if bytes_written < 4096 {
                    // SAFETY: Zero remaining bytes of the demand page.
                    unsafe {
                        core::ptr::write_bytes(
                            dest_ptr.add(bytes_written),
                            0,
                            4096 - bytes_written,
                        );
                    }
                }
                frame
            }
        };

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
