// SPDX-License-Identifier: GPL-2.0

//! `msync(2)` implementation.

use core::ops::Range;
use ostd::mm::{PAGE_SIZE, Vaddr};

use super::super::{Vmar, is_mappable_range, mappings::VmMapping};
use crate::{api::errno::Result, vm::flags::MsyncFlags};

impl Vmar {
    /// Brings the mapping of `range` in line with the object behind it.
    pub fn msync(&self, flags: MsyncFlags, addr: Vaddr, len: usize) -> Result<()> {
        flags.validate()?;

        if !addr.is_multiple_of(PAGE_SIZE) {
            crate::return_errno!(EINVAL, "the address {addr:#x} is not page-aligned");
        }
        let Some(len) = len.checked_next_multiple_of(PAGE_SIZE) else {
            crate::return_errno!(EINVAL, "a length of {len} bytes from {addr:#x} overflows");
        };
        if len == 0 {
            return Ok(());
        }
        let Some(end) = addr.checked_add(len) else {
            crate::return_errno!(EINVAL, "a length of {len} bytes from {addr:#x} overflows");
        };
        let range = addr..end;

        if !is_mappable_range(&range) {
            crate::return_errno!(
                ENOMEM,
                "the range {range:#x?} is not inside the user address space"
            );
        }
        {
            let inner = self.inner.read();
            if !inner.mappings.is_fully_mapped(&range) {
                let missing = range.len() - inner.mappings.count_overlap(&range);
                crate::return_errno!(
                    ENOMEM,
                    "{missing:#x} of the {}-byte range are not mapped",
                    range.len()
                );
            }
        }

        let mappings: alloc::vec::Vec<VmMapping> = self
            .inner
            .read()
            .mappings
            .iter_range(&range)
            .map(|mapping| mapping.dup())
            .collect();

        for mapping in &mappings {
            if let Some(backing) = mapping.backing()
                && let Some(object) = object_range(mapping, &range, backing.size())
            {
                backing.flush(object)?;
            }
        }

        if flags.contains(MsyncFlags::INVALIDATE) {
            let mut inner = self.inner.write();
            for mapping in &mappings {
                let part = super::super::intersect(&range, &mapping.range());
                inner.unmap_pages_only(&self.vm_space, &part);
            }
        }
        Ok(())
    }
}

fn object_range(
    mapping: &VmMapping,
    wanted: &Range<Vaddr>,
    object_size: usize,
) -> Option<Range<usize>> {
    let part = super::super::intersect(wanted, &mapping.range());
    let start = mapping.backing_offset() + (part.start - mapping.start());
    let backed = start..start + part.len().min(object_size.saturating_sub(start));
    (!backed.is_empty()).then_some(backed)
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;

    #[ktest]
    fn repro_msync_flags_sync_plus_invalidate() {
        let raw = (MsyncFlags::SYNC | MsyncFlags::INVALIDATE).bits();
        let parsed = MsyncFlags::from_raw(raw);
        assert!(
            parsed.is_ok(),
            "MS_SYNC | MS_INVALIDATE must be valid per POSIX, got {:?}",
            parsed
        );
    }
}
