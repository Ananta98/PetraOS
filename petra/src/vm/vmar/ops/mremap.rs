// SPDX-License-Identifier: GPL-2.0

//! `mremap(2)` and mapping resize implementation.

use crate::{uapi::errno::Result, vm::flags::MremapFlags};
use core::ops::Range;
use ostd::mm::{PAGE_SIZE, Vaddr, vm_space::VmQueriedItem};

use super::super::{
    Vmar, VmarInner, check_page_aligned_range, is_intersected, is_mappable_range,
    mappings::VmMapping,
};

impl Vmar {
    /// Moves or resizes the mapping that starts at `old_addr`.
    pub fn mremap(
        &self,
        old_addr: Vaddr,
        old_size: usize,
        new_size: usize,
        flags: MremapFlags,
        new_addr: Option<Vaddr>,
    ) -> Result<Vaddr> {
        let old_size = page_aligned_size(old_size, "old_size")?;
        let new_size = page_aligned_size(new_size, "new_size")?;
        if !old_addr.is_multiple_of(PAGE_SIZE) {
            crate::return_errno!(EINVAL, "the old address {old_addr:#x} is not page-aligned");
        }
        if let Some(new_addr) = new_addr
            && !new_addr.is_multiple_of(PAGE_SIZE)
        {
            crate::return_errno!(EINVAL, "the new address {new_addr:#x} is not page-aligned");
        }

        let fixed = flags.contains(MremapFlags::FIXED);
        let keep_old = flags.contains(MremapFlags::DONTUNMAP);
        if fixed && new_addr.is_none() {
            crate::return_errno!(EINVAL, "MREMAP_FIXED requires a new address");
        }
        if keep_old && fixed {
            crate::return_errno!(
                EINVAL,
                "MREMAP_DONTUNMAP cannot be combined with MREMAP_FIXED"
            );
        }
        if !flags.contains(MremapFlags::MAYMOVE) && new_size > old_size {
            crate::return_errno!(
                EINVAL,
                "growing from {old_size} to {new_size} bytes requires MREMAP_MAYMOVE"
            );
        }
        if keep_old && new_size < old_size {
            crate::return_errno!(
                EINVAL,
                "MREMAP_DONTUNMAP cannot shrink {old_size} to {new_size} bytes"
            );
        }

        let new_addr = if fixed { new_addr } else { None };

        let old_range = old_addr..old_addr + old_size;
        if !is_mappable_range(&old_range) {
            crate::return_errno!(EFAULT, "the old range {old_range:#x?} is not in user space");
        }
        if !self.is_fully_mapped(&old_range) {
            crate::return_errno!(EFAULT, "the old range {old_range:#x?} is not fully mapped");
        }
        if let Some(new_addr) = new_addr {
            let new_range = new_addr..new_addr + new_size;
            if !is_mappable_range(&new_range) {
                crate::return_errno!(ENOMEM, "the new range {new_range:#x?} is not in user space");
            }
            if is_intersected(&old_range, &new_range) {
                crate::return_errno!(
                    EINVAL,
                    "the new range {new_range:#x?} overlaps the old one {old_range:#x?}"
                );
            }
        }

        let mut inner = self.inner.write();
        let before = inner.total_vm;

        if keep_old {
            let new_start = match new_addr {
                Some(new_start) => new_start,
                None => {
                    inner
                        .find_free_region(new_size, inner.default_high_limit())?
                        .start
                }
            };
            return Self::remap_keep_old(self, &mut inner, &old_range, new_start, new_size);
        }

        if new_size < old_size {
            inner.unmap_range(&self.vm_space, &(old_addr + new_size..old_addr + old_size));
        }
        let kept = old_size.min(new_size);

        if new_addr.is_none() {
            if new_size <= old_size {
                inner.total_vm = before - old_size + new_size;
                return Ok(old_addr);
            }

            if new_size > old_size {
                let grow = old_addr + old_size..old_addr + new_size;
                let usable = is_mappable_range(&grow) && inner.mappings.count_overlap(&grow) == 0;
                if usable
                    && let Some(key) = inner.mappings.get(old_range.end - 1).map(VmMapping::start)
                {
                    inner.check_total_fits(self, before - old_size + new_size)?;
                    let mapping = inner
                        .mappings
                        .remove(key)
                        .expect("a key came from this very set");
                    inner.mappings.insert(mapping.enlarge(new_size - old_size));
                    inner.total_vm = before - old_size + new_size;
                    return Ok(old_addr);
                }
            }
        }

        let new_start = match new_addr {
            Some(new_addr) => new_addr,
            None => {
                inner
                    .find_free_region(new_size, inner.default_high_limit())?
                    .start
            }
        };
        let new_range = new_start..new_start + new_size;
        inner.check_total_fits(self, before - old_size + new_size)?;
        inner.unmap_range(&self.vm_space, &new_range);

        let moved = old_range.start..old_range.start + kept;
        self.move_pt(&moved, &(new_start..new_start + kept));

        let grow_by = new_size - kept;
        let grown_end = new_start + kept;
        inner.carve(&moved, |piece, sub| {
            let delta = sub.start - moved.start;
            let mut piece = piece.relocate(new_start + delta);
            piece.rebase(delta);
            if piece.end() == grown_end {
                piece = piece.enlarge(grow_by);
            }
            Some(piece)
        });
        inner.total_vm = before - old_size + new_size;
        Ok(new_start)
    }

    fn remap_keep_old(
        vmar: &Vmar,
        inner: &mut VmarInner,
        old_range: &Range<Vaddr>,
        new_start: Vaddr,
        new_size: usize,
    ) -> Result<Vaddr> {
        let source = inner
            .mappings
            .get(old_range.start)
            .map(VmMapping::dup)
            .ok_or(crate::uapi::errno::Errno::EFAULT)?;
        if source.end() < old_range.end {
            crate::return_errno!(
                EFAULT,
                "the old range {old_range:#x?} spans more than one mapping"
            );
        }

        let new_range = new_start..new_start + new_size;
        if !is_mappable_range(&new_range) {
            crate::return_errno!(ENOMEM, "the new range {new_range:#x?} is not in user space");
        }
        let before = inner.total_vm;
        inner.check_total_fits(vmar, before + new_size)?;
        inner.unmap_range(&vmar.vm_space, &new_range);

        {
            let span = old_range.start.min(new_start)..old_range.end.max(new_range.end);
            let guard = ostd::task::disable_preempt();
            let mut cursor = vmar
                .vm_space
                .cursor_mut(&guard, &span)
                .expect("the ranges lie inside the VM space");
            let mut offset = 0;
            while offset < old_range.len() {
                cursor
                    .jump(old_range.start + offset)
                    .expect("the old range is page-aligned");
                let Some(mapped) = cursor.find_next(old_range.len() - offset) else {
                    break;
                };
                let (_, Some(item)) = cursor.query().expect("a mapped page was just found") else {
                    break;
                };
                let at_new = new_start + (mapped - old_range.start);
                match item {
                    VmQueriedItem::MappedRam { frame, prop } => {
                        let frame = (*frame).clone();
                        cursor.jump(at_new).expect("the new range is page-aligned");
                        cursor.map(frame, prop);
                    }
                    VmQueriedItem::MappedIoMem { .. } => {
                        unreachable!("device mappings are not supported")
                    }
                }
                offset = (mapped - old_range.start) + PAGE_SIZE;
            }
            cursor.flusher().dispatch_tlb_flush();
            cursor.flusher().sync_tlb_flush();
        }

        let mapping = source
            .relocate(new_start)
            .enlarge(new_size - old_range.len());
        inner.mappings.insert(mapping);
        inner.total_vm = before + new_size;
        Ok(new_start)
    }

    fn move_pt(&self, old_range: &Range<Vaddr>, new_range: &Range<Vaddr>) {
        debug_assert_eq!(old_range.len(), new_range.len());
        debug_assert!(!is_intersected(old_range, new_range));

        let guard = ostd::task::disable_preempt();
        let span = old_range.start.min(new_range.start)..old_range.end.max(new_range.end);
        let mut cursor = self
            .vm_space
            .cursor_mut(&guard, &span)
            .expect("the ranges lie inside the VM space");

        let mut offset = 0;
        while offset < old_range.len() {
            cursor
                .jump(old_range.start + offset)
                .expect("the old range is page-aligned");
            let Some(mapped) = cursor.find_next(old_range.len() - offset) else {
                break;
            };
            let (va, Some(item)) = cursor.query().expect("a mapped page was just found") else {
                break;
            };
            debug_assert_eq!(mapped, va.start);

            let at_new = new_range.start + (mapped - old_range.start);
            match item {
                VmQueriedItem::MappedRam { frame, prop } => {
                    let frame = (*frame).clone();
                    cursor.unmap(PAGE_SIZE);
                    cursor.jump(at_new).expect("the new range is page-aligned");
                    cursor.map(frame, prop);
                }
                VmQueriedItem::MappedIoMem { .. } => {
                    unreachable!("device mappings are not supported")
                }
            }
            offset = (mapped - old_range.start) + PAGE_SIZE;
        }

        cursor.flusher().dispatch_tlb_flush();
        cursor.flusher().sync_tlb_flush();
    }

    /// Resizes the mapping that covers `addr..addr + old_size` to `new_size` bytes.
    pub fn resize_mapping(&self, addr: Vaddr, old_size: usize, new_size: usize) -> Result<()> {
        let old_size = page_aligned_size(old_size, "old_size")?;
        let new_size = page_aligned_size(new_size, "new_size")?;
        let old_range = addr..addr + old_size;
        check_page_aligned_range(&old_range)?;
        check_page_aligned_range(&(addr..addr + new_size))?;

        let mut inner = self.inner.write();

        let owner = inner
            .mappings
            .get(addr)
            .map(|mapping| (mapping.start(), mapping.end()))
            .ok_or(crate::uapi::errno::Errno::EFAULT)?;
        if owner.1 < old_range.end {
            crate::return_errno!(
                EFAULT,
                "the range {old_range:#x?} spans more than one mapping"
            );
        }

        if new_size == old_size {
            return Ok(());
        }
        if new_size < old_size {
            inner.unmap_range(&self.vm_space, &(addr + new_size..old_range.end));
            return Ok(());
        }

        let grow = old_range.end..addr + new_size;
        if !is_mappable_range(&grow) {
            crate::return_errno!(ENOMEM, "the range {grow:#x?} is not in user space");
        }
        if inner.mappings.count_overlap(&grow) != 0 {
            crate::return_errno!(EFAULT, "the range {grow:#x?} is already mapped");
        }
        inner.check_total_fits(self, inner.total_vm + (new_size - old_size))?;

        let mapping = inner
            .mappings
            .remove(owner.0)
            .expect("the owner was just found");
        inner.mappings.insert(mapping.enlarge(new_size - old_size));
        inner.total_vm += new_size - old_size;
        Ok(())
    }
}

fn page_aligned_size(size: usize, what: &str) -> Result<usize> {
    if size == 0 {
        crate::return_errno!(EINVAL, "{what} must not be zero");
    }
    match size.checked_next_multiple_of(PAGE_SIZE) {
        Some(size) => Ok(size),
        None => crate::return_errno!(ENOMEM, "{what} of {size} bytes overflows"),
    }
}
