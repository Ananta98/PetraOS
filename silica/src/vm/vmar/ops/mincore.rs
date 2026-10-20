// SPDX-License-Identifier: GPL-2.0

//! `mincore(2)` virtual memory page table residency query.

use core::ops::Range;
use ostd::mm::{PAGE_SIZE, Vaddr};

use super::super::{Vmar, check_page_aligned_range};
use crate::api::errno::Result;

impl Vmar {
    /// Determines whether pages in `range` are resident in physical memory.
    ///
    /// For each 4 KiB page, sets the least significant bit of `vec_out[i]` to `1`
    /// if the page is currently present in the page table, or `0` if not resident.
    pub fn mincore(&self, range: Range<Vaddr>, vec_out: &mut [u8]) -> Result<()> {
        check_page_aligned_range(&range)?;
        let num_pages = range.len() / PAGE_SIZE;
        if vec_out.len() < num_pages {
            crate::return_errno!(EINVAL, "output vector buffer is too small");
        }

        {
            let inner = self.inner.read();
            if !inner.mappings.is_fully_mapped(&range) {
                crate::return_errno!(ENOMEM, "address range {range:#x?} contains unmapped holes");
            }
        }

        let guard = ostd::task::disable_preempt();
        let mut cursor = self.vm_space.cursor(&guard, &range)?;

        for (i, page_addr) in (range.start..range.end).step_by(PAGE_SIZE).enumerate() {
            cursor.jump(page_addr)?;
            let (_, item) = cursor.query()?;
            vec_out[i] = if item.is_some() { 1 } else { 0 };
        }

        Ok(())
    }
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use crate::vm::abi::{flags::MmapFlags, perms::VmPerms};
    use ostd::mm::PAGE_SIZE;
    use ostd::prelude::ktest;

    #[ktest]
    fn mincore_detects_residency() {
        let vmar = Vmar::new();
        let addr = 0x6000_0000;
        let len = 2 * PAGE_SIZE;
        vmar.mmap_anonymous(
            addr,
            len,
            VmPerms::READ | VmPerms::WRITE,
            MmapFlags::PRIVATE | MmapFlags::FIXED | MmapFlags::ANONYMOUS,
        )
        .unwrap();

        // Populate only page 0
        vmar.populate_range(&(addr..addr + PAGE_SIZE)).unwrap();

        let mut vec = [0u8; 2];
        vmar.mincore(addr..addr + len, &mut vec).unwrap();

        assert_eq!(vec[0] & 1, 1, "page 0 must be resident");
        assert_eq!(vec[1] & 1, 0, "page 1 must not be resident");
    }
}
