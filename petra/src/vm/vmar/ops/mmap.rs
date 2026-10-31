// SPDX-License-Identifier: GPL-2.0

//! `mmap(2)` implementation and batched page population.

use alloc::sync::Arc;
use core::num::NonZeroUsize;

use ostd::mm::{PAGE_SIZE, Vaddr, tlb::TlbFlushOp};
use ostd::task::disable_preempt;

use crate::{
    uapi::errno::Result,
    vm::{backing::Backing, flags::MmapFlags, perms::VmPerms},
};

use super::super::{
    VMAR_CAP_ADDR, VMAR_LOWEST_ADDR, Vmar, check_page_aligned_range, is_mappable_range,
    mappings::VmMapping,
};

impl Vmar {
    /// Maps `len` bytes of anonymous memory.
    pub fn mmap_anonymous(
        &self,
        addr: Vaddr,
        len: usize,
        prot: VmPerms,
        flags: MmapFlags,
    ) -> Result<Vaddr> {
        let backing = if flags.is_shared() {
            Some(Arc::clone(&crate::vm::backing::shm::new_anonymous(len)?) as Arc<dyn Backing>)
        } else {
            None
        };
        self.mmap_inner(backing, addr, len, prot, flags, 0)
    }

    /// Maps `len` bytes of `backing`, starting at `offset` within it.
    pub fn mmap_backed(
        &self,
        backing: &Arc<dyn Backing>,
        addr: Vaddr,
        len: usize,
        prot: VmPerms,
        flags: MmapFlags,
        offset: usize,
    ) -> Result<Vaddr> {
        self.mmap_inner(Some(Arc::clone(backing)), addr, len, prot, flags, offset)
    }

    /// Maps the virtual Dynamic Shared Object (vDSO) and its associated `vvar` data page
    /// into this address space.
    ///
    /// Following Linux x86_64 layout (as in `asterinas/linux_vdso`), the `vvar` page is mapped
    /// read-only at `vdso_base - 4 * PAGE_SIZE`, while the `vdso` code page is mapped read-exec
    /// at `vdso_base`.
    ///
    /// Returns the virtual base address of the vDSO shared library (`vdso_base`).
    pub fn map_vdso(&self) -> Result<Vaddr> {
        #[cfg(target_arch = "x86_64")]
        {
            use crate::arch::vdso::{
                SingleFrameBacking, VDSO_DATA_SEGMENT_OFFSET, VDSO_DATA_SEGMENT_SIZE,
                VDSO_TEXT_SEGMENT_OFFSET, VDSO_TEXT_SEGMENT_SIZE, VDSO_TOTAL_SIZE,
            };

            let (vvar_backing, vdso_backing) = crate::arch::vdso::get_backings()?;
            let total_size = VDSO_TOTAL_SIZE;

            let high_limit = {
                let inner = self.inner.read();
                inner.default_high_limit()
            };

            let free_range = {
                let inner = self.inner.read();
                inner
                    .find_free_region(total_size, high_limit)
                    .map_err(|_| crate::uapi::errno::Errno::ENOMEM)?
            };

            let vvar_addr = free_range.start + VDSO_DATA_SEGMENT_OFFSET;
            let vdso_addr = free_range.start + VDSO_TEXT_SEGMENT_OFFSET;

            // 1. Map vvar data page (read-only) at offset 0
            self.mmap_backed(
                &vvar_backing,
                vvar_addr,
                VDSO_DATA_SEGMENT_SIZE,
                VmPerms::READ,
                MmapFlags::PRIVATE | MmapFlags::FIXED | MmapFlags::POPULATE,
                0,
            )?;

            // 2. Map placeholder zero pages for the intermediate reserved/timens region
            let zero_frame = crate::vm::backing::get_zero_frame()?;
            let zero_backing: Arc<dyn Backing> = Arc::new(SingleFrameBacking(zero_frame));
            let middle_start = vvar_addr + VDSO_DATA_SEGMENT_SIZE;
            let middle_size = VDSO_TEXT_SEGMENT_OFFSET - VDSO_DATA_SEGMENT_SIZE;
            for offset in (0..middle_size).step_by(PAGE_SIZE) {
                self.mmap_backed(
                    &zero_backing,
                    middle_start + offset,
                    PAGE_SIZE,
                    VmPerms::READ,
                    MmapFlags::PRIVATE | MmapFlags::FIXED | MmapFlags::POPULATE,
                    0,
                )?;
            }

            // 3. Map vDSO ELF code segment (read+exec) at offset 4 * PAGE_SIZE
            self.mmap_backed(
                &vdso_backing,
                vdso_addr,
                VDSO_TEXT_SEGMENT_SIZE,
                VmPerms::READ | VmPerms::EXEC,
                MmapFlags::PRIVATE | MmapFlags::FIXED | MmapFlags::POPULATE,
                0,
            )?;

            Ok(vdso_addr)
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            crate::return_errno!(ENOSYS, "vDSO is only supported on x86_64");
        }
    }

    /// The body of `mmap(2)`, shared by both wrappers above.
    fn mmap_inner(
        &self,
        backing: Option<Arc<dyn Backing>>,
        addr: Vaddr,
        len: usize,
        prot: VmPerms,
        flags: MmapFlags,
        offset: usize,
    ) -> Result<Vaddr> {
        if len == 0 {
            crate::return_errno!(EINVAL, "cannot map zero bytes");
        }
        let Some(len) = len.checked_next_multiple_of(PAGE_SIZE) else {
            crate::return_errno!(ENOMEM, "the length {len} overflows");
        };
        if !addr.is_multiple_of(PAGE_SIZE) {
            crate::return_errno!(EINVAL, "the address {addr:#x} is not page-aligned");
        }
        if !offset.is_multiple_of(PAGE_SIZE) {
            crate::return_errno!(EINVAL, "the offset {offset:#x} is not page-aligned");
        }
        flags.validate()?;
        if prot != prot.granted() {
            crate::return_errno!(EINVAL, "{prot:?} is not a valid protection");
        }

        // Anonymous memory or private (CoW) file mappings are owned by the process,
        // so grant full ceiling (MAY_READ | MAY_WRITE | MAY_EXEC) allowing subsequent mprotect.
        let perms = if backing.is_none() || !flags.is_shared() {
            prot.with_full_ceiling()
        } else {
            prot.with_ceiling()
        };
        perms.check()?;

        let Some(size) = NonZeroUsize::new(len) else {
            crate::return_errno!(EINVAL, "cannot map zero bytes");
        };

        if let Some(backing) = &backing {
            let Some(end) = offset.checked_add(len) else {
                crate::return_errno!(
                    EOVERFLOW,
                    "the offset {offset:#x} plus {len} bytes overflows"
                );
            };
            if end > backing.size() {
                crate::return_errno!(
                    EINVAL,
                    "the range {offset:#x}..{end:#x} is past the end of the {:#x}-byte object",
                    backing.size()
                );
            }
        }

        let mut inner = self.inner.write();
        let high_limit = inner.default_high_limit();

        let start = if flags.contains(MmapFlags::FIXED) {
            addr
        } else if flags.contains(MmapFlags::FIXED_NOREPLACE) {
            let wanted = addr..addr + len;
            if inner.mappings.count_overlap(&wanted) != 0 {
                crate::return_errno!(
                    EEXIST,
                    "MAP_FIXED_NOREPLACE: {addr:#x}..{:#x} is already mapped",
                    wanted.end
                );
            }
            addr
        } else {
            #[cfg(target_arch = "x86_64")]
            let search_high = if flags.contains(MmapFlags::MAP_32BIT) {
                super::super::MAP_32BIT_HIGH_LIMIT.min(high_limit)
            } else {
                high_limit
            };
            #[cfg(not(target_arch = "x86_64"))]
            let search_high = high_limit;

            let hint = (addr != 0).then(|| addr..addr + len);
            match hint {
                Some(hint)
                    if is_mappable_range(&hint) && inner.mappings.count_overlap(&hint) == 0 =>
                {
                    hint.start
                }
                _ => inner.find_free_region(len, search_high)?.start,
            }
        };

        let range = start..start + len;
        if !is_mappable_range(&range) {
            crate::return_errno!(
                ENOMEM,
                "{:#x}..{:#x} is not inside the user address space {VMAR_LOWEST_ADDR:#x}..{VMAR_CAP_ADDR:#x}",
                range.start,
                range.end
            );
        }
        if !flags.contains(MmapFlags::FIXED) {
            inner.check_fits_addr_space(self, len)?;
        }

        inner.unmap_range(&self.vm_space, &range);

        let mut mapping = VmMapping::new(
            start,
            size,
            perms,
            flags.is_shared(),
            flags.grows_down(),
            backing,
            offset,
        );
        if flags.is_locked() {
            mapping.set_locked(true);
        }

        inner.total_vm += len;
        inner.mappings.insert(mapping);
        drop(inner);

        if flags.contains(MmapFlags::POPULATE) {
            self.populate_range(&range)?;
        }
        Ok(start)
    }

    /// Faults in every page of the mapped part of `range` using batched mapping.
    pub fn populate_range(&self, range: &core::ops::Range<Vaddr>) -> Result<()> {
        check_page_aligned_range(range)?;

        let mappings: alloc::vec::Vec<VmMapping> = self
            .inner
            .read()
            .mappings
            .iter_range(range)
            .map(|m| m.dup())
            .collect();

        for mapping in mappings {
            let wanted = super::super::intersect(range, &mapping.range());
            let perms = mapping.perms().granted();
            let is_write = perms.contains(VmPerms::WRITE);

            let guard = disable_preempt();
            let Ok(mut cursor) = self.vm_space.cursor_mut(&guard, &wanted) else {
                continue;
            };

            let mut addr = wanted.start;
            let mut modified = false;

            while addr < wanted.end {
                cursor.jump(addr).ok();
                let (_, item) = cursor.query().expect("page-aligned");
                if item.is_none() {
                    let offset = addr - mapping.start();
                    let frame = match mapping.backing() {
                        Some(backing) if mapping.is_backed(offset) => {
                            backing.frame(mapping.backing_offset_of(offset))?
                        }
                        _ => {
                            if !is_write && !mapping.is_shared() {
                                crate::vm::backing::zero::get_zero_frame()?
                            } else {
                                crate::vm::backing::alloc_zeroed_frame()?
                            }
                        }
                    };
                    let is_shared_zero = crate::vm::backing::zero::is_zero_frame(&frame);
                    let must_copy =
                        mapping.is_cow() && (frame.reference_count() > 1 || is_shared_zero);
                    let property = mapping.fault_property(!must_copy, is_write);
                    cursor.map(frame, property);
                    modified = true;
                }
                addr += PAGE_SIZE;
            }

            if modified {
                cursor
                    .flusher()
                    .issue_tlb_flush(TlbFlushOp::for_range(wanted.clone()));
                cursor.flusher().dispatch_tlb_flush();
                cursor.flusher().sync_tlb_flush();
            }
        }
        Ok(())
    }
}
