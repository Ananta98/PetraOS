//! Virtual Memory Synchronization (`msync`) for PetraOS.
//!
//! Synchronizes changes made to a memory-mapped file or region back to storage.
//! Implements standard Linux POSIX semantics: `MS_ASYNC`, `MS_INVALIDATE`, and `MS_SYNC`.

use crate::mm::vmm::paging::{PageTable, VirtAddr};
use crate::mm::vmm::types::VmAreaKind;
use crate::mm::vmm::vma::{AddrSpace, AddrSpaceError};
use alloc::sync::Arc;
use alloc::vec::Vec;

/// Perform asynchronous write: return immediately once write requests are submitted.
pub const MS_ASYNC: i32 = 1;
/// Invalidate cached copies of mapped data.
pub const MS_INVALIDATE: i32 = 2;
/// Perform synchronous write: wait until data is fully written and synced to disk.
pub const MS_SYNC: i32 = 4;

impl<P: PageTable> AddrSpace<P> {
    /// Synchronize changes made to the mapped memory range `[start, start + size)`.
    ///
    /// # Semantics (Linux ABI)
    /// 1. `start` must be aligned to 4096 bytes (page boundary).
    /// 2. If `size == 0`, returns `Ok(())` immediately.
    /// 3. `flags` must contain either `MS_ASYNC` or `MS_SYNC`, but never both.
    /// 4. Undefined flag bits cause `AddrSpaceError::InvalidRange` (maps to `EINVAL`).
    /// 5. Every page in `[start, start + aligned_size)` must be covered by existing VMAs.
    ///    Any unmapped hole causes `AddrSpaceError::UnmappedRange` (maps to `ENOMEM`).
    /// 6. For `VmAreaKind::File`:
    ///    - Writes dirty/mapped pages back to the underlying `file`.
    ///    - If `MS_SYNC` is set, calls `file.sync()` to flush blocks to device.
    /// 7. For anonymous memory (`VmAreaKind::Anonymous`), `msync` succeeds as a valid no-op.
    pub fn msync_range(
        &mut self,
        start: VirtAddr,
        size: usize,
        flags: i32,
    ) -> Result<(), AddrSpaceError> {
        if size == 0 {
            return Ok(());
        }

        if !start.is_aligned(4096u64) {
            return Err(AddrSpaceError::InvalidRange);
        }

        // Validate flags: must be subset of MS_ASYNC | MS_INVALIDATE | MS_SYNC
        let allowed_flags = MS_ASYNC | MS_INVALIDATE | MS_SYNC;
        if (flags & !allowed_flags) != 0 {
            return Err(AddrSpaceError::InvalidRange);
        }

        // Exactly one of MS_ASYNC or MS_SYNC must be specified
        let sync_or_async = flags & (MS_ASYNC | MS_SYNC);
        if sync_or_async == 0 || sync_or_async == (MS_ASYNC | MS_SYNC) {
            return Err(AddrSpaceError::InvalidRange);
        }

        let aligned_size = (size + 4095) & !4095;
        let end = match start.as_u64().checked_add(aligned_size as u64) {
            Some(e) => VirtAddr::new(e),
            None => return Err(AddrSpaceError::InvalidRange),
        };

        // 1. Verify that the entire range [start, end) is completely covered by existing VMAs.
        let mut curr = start;
        while curr < end {
            if let Some(vma) = self.find_vma(curr) {
                if vma.end <= curr {
                    return Err(AddrSpaceError::UnmappedRange);
                }
                curr = vma.end;
            } else {
                return Err(AddrSpaceError::UnmappedRange);
            }
        }

        // 2. Iterate through VMAs and sync any file-backed pages
        let mut files_to_sync: Vec<Arc<dyn crate::fs::FileOps>> = Vec::new();

        for page_virt_u64 in (start.as_u64()..end.as_u64()).step_by(4096) {
            let page_virt = VirtAddr::new(page_virt_u64);
            if let Some(vma) = self.find_vma(page_virt) {
                if let VmAreaKind::File {
                    file,
                    offset,
                    file_size,
                } = &vma.kind
                {
                    if let Some((phys_frame, _)) = self.page_table.get_entry(page_virt) {
                        let page_file_offset = *offset + (page_virt - vma.start) as usize;
                        if page_file_offset < *file_size {
                            let bytes_to_write = core::cmp::min(4096, *file_size - page_file_offset);
                            let hhdm = crate::mm::hhdm_offset();
                            let src_ptr = (phys_frame.as_u64() + hhdm) as *const u8;
                            // SAFETY: Frame is mapped to page_virt and accessible via HHDM.
                            let buf_slice =
                                unsafe { core::slice::from_raw_parts(src_ptr, bytes_to_write) };
                            let _ = file.write(page_file_offset, buf_slice);
                        }
                    }

                    if (flags & MS_SYNC) != 0 {
                        if !files_to_sync.iter().any(|f| Arc::ptr_eq(f, file)) {
                            files_to_sync.push(Arc::clone(file));
                        }
                    }
                }
            }

            if (flags & MS_INVALIDATE) != 0 {
                self.page_table.flush_tlb(page_virt);
            }
        }

        // 3. If synchronous, ensure all touched files are flushed to disk
        if (flags & MS_SYNC) != 0 {
            for file in files_to_sync {
                let _ = file.sync();
            }
        }

        Ok(())
    }
}
