//! Architecture-Independent Page Fault Handler for PetraOS.
//!
//! Evaluates virtual memory access violations, resolves demand paging,
//! and handles Copy-On-Write (COW) page duplication.

use crate::mm::vmm::paging::{
    COW_FLAG, PageFaultErrorCode, PageTable, PageTableFlags, PagingError, VirtAddr,
};
use crate::mm::vmm::types::VmAreaKind;
use crate::mm::vmm::vma::AddrSpace;

/// Errors that can occur during page fault resolution.
#[derive(Debug)]
pub enum PageFaultError {
    /// Virtual address is not within any registered VMA.
    UnmappedAccess,
    /// VMA flags disallow the requested access mode (e.g. write to read-only).
    ProtectionViolation,
    /// Physical memory allocator ran out of pages.
    FrameAllocationFailed,
    /// Failure while mapping/unmapping page tables.
    PagingError(PagingError),
    /// Failure while updating flags on an existing page table entry.
    RemapError(PagingError),
}

impl<P: PageTable> AddrSpace<P> {
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
            VmAreaKind::Shared { shmid } => {
                let page_index = ((page_virt - area.start) / 4096) as usize;
                let mgr = crate::ipc::shm::SHM_MANAGER.lock();
                let seg = mgr.segments.get(shmid).ok_or(PageFaultError::UnmappedAccess)?;
                let frame = seg.frames.get(page_index).copied().ok_or(PageFaultError::UnmappedAccess)?;
                crate::mm::PMM.inc_ref(frame);
                frame
            }
        };

        self.page_table
            .map(page_virt, frame_phys, area.flags)
            .map_err(PageFaultError::PagingError)?;

        Ok(())
    }
}
