// SPDX-License-Identifier: GPL-2.0

//! `mlock(2)` and `munlock(2)` virtual memory address space operations.

use core::ops::Range;
use ostd::mm::Vaddr;

use super::super::{Vmar, is_mappable_range};
use crate::uapi::errno::Result;

impl Vmar {
    /// Marks the memory range as locked into RAM or unlocked.
    pub fn mlock_range(&self, range: Range<Vaddr>, locked: bool) -> Result<()> {
        if !is_mappable_range(&range) {
            crate::return_errno!(
                ENOMEM,
                "the range {range:#x?} is not in the user address space"
            );
        }
        let mut inner = self.inner.write();
        inner.carve(&range, |mut piece, _| {
            piece.set_locked(locked);
            Some(piece)
        });
        Ok(())
    }
}
