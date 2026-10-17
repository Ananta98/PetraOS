// SPDX-License-Identifier: GPL-2.0

//! `munmap(2)` and `madvise(2)` implementation.

use core::ops::Range;
use ostd::mm::Vaddr;

use crate::{api::errno::Result, vm::flags::MadviseAdvice};
use super::super::{Vmar, check_page_aligned_range, is_mappable_range};

impl Vmar {
    /// Releases the mappings that intersect `range`.
    pub fn munmap(&self, range: Range<Vaddr>) -> Result<()> {
        check_page_aligned_range(&range)?;
        if range.end <= range.start {
            crate::return_errno!(EINVAL, "the range wraps around the address space or is empty");
        }

        let mut inner = self.inner.write();
        inner.unmap_range(&self.vm_space, &range);
        Ok(())
    }

    /// Gives the kernel a hint about how `range` will be used.
    pub fn madvise(&self, advice: MadviseAdvice, range: Range<Vaddr>) -> Result<()> {
        check_page_aligned_range(&range)?;

        if advice == MadviseAdvice::WillNeed {
            let _ = self.populate_range(&range);
            return Ok(());
        }

        // Fork controls: update mappings in the range
        match advice {
            MadviseAdvice::DontFork => {
                let mut inner = self.inner.write();
                inner.carve(&range, |mut piece, _| {
                    piece.set_dont_fork(true);
                    Some(piece)
                });
                return Ok(());
            }
            MadviseAdvice::DoFork => {
                let mut inner = self.inner.write();
                inner.carve(&range, |mut piece, _| {
                    piece.set_dont_fork(false);
                    Some(piece)
                });
                return Ok(());
            }
            MadviseAdvice::WipeOnFork => {
                let mut inner = self.inner.write();
                inner.carve(&range, |mut piece, _| {
                    piece.set_wipe_on_fork(true);
                    Some(piece)
                });
                return Ok(());
            }
            MadviseAdvice::KeepOnFork => {
                let mut inner = self.inner.write();
                inner.carve(&range, |mut piece, _| {
                    piece.set_wipe_on_fork(false);
                    Some(piece)
                });
                return Ok(());
            }
            MadviseAdvice::HugePage | MadviseAdvice::NoHugePage | MadviseAdvice::Mergeable | MadviseAdvice::Unmergeable => {
                return Ok(());
            }
            _ => {}
        }

        if !advice.drops_pages() {
            return Ok(());
        }

        if !is_mappable_range(&range) {
            crate::return_errno!(
                EFAULT,
                "the range {range:#x?} is not in the user address space"
            );
        }

        let mut inner = self.inner.write();
        let outcome = inner.unmap_pages_only(&self.vm_space, &range);

        if outcome.mapped < range.len() {
            crate::return_errno!(
                ENOMEM,
                "{:#x} of the {}-byte range are not mapped",
                range.len() - outcome.mapped,
                range.len()
            );
        }
        Ok(())
    }
}
