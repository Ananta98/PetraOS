// SPDX-License-Identifier: GPL-2.0

//! `mprotect(2)` implementation.

use core::ops::Range;
use ostd::mm::Vaddr;

use super::super::{Vmar, check_page_aligned_range, is_mappable_range};
use crate::{uapi::errno::Result, vm::perms::VmPerms};

impl Vmar {
    /// Replaces the permissions in force over `range`.
    pub fn mprotect(&self, prot: VmPerms, range: Range<Vaddr>) -> Result<()> {
        check_page_aligned_range(&range)?;
        if prot != prot.granted() {
            crate::return_errno!(EINVAL, "{prot:?} is not a valid protection");
        }
        if !is_mappable_range(&range) {
            crate::return_errno!(
                EFAULT,
                "the range {range:#x?} is not in the user address space"
            );
        }

        {
            let inner = self.inner.read();
            for mapping in inner.mappings.iter_range(&range) {
                if !mapping.perms().allows(prot) {
                    crate::return_errno!(
                        EACCES,
                        "{prot:?} exceeds what the mapping at {:#x} may be protected to ({:?})",
                        mapping.start(),
                        mapping.perms().may()
                    );
                }
            }
        }

        let mut inner = self.inner.write();
        let vm_space = &self.vm_space;
        let outcome = inner.carve(&range, |mut piece, _| {
            piece.set_perms(piece.perms().with_granted(prot));
            piece.protect(vm_space, piece.perms());
            Some(piece)
        });

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

#[cfg(ktest)]
mod tests {
    use super::*;
    use crate::vm::abi::{flags::MmapFlags, perms::VmPerms};
    use ostd::mm::PAGE_SIZE;
    use ostd::prelude::ktest;

    #[ktest]
    fn repro_mmap_prot_none_then_mprotect_fails() {
        let vmar = Vmar::new();
        let addr = vmar
            .mmap_anonymous(
                0x3000_0000,
                PAGE_SIZE,
                VmPerms::empty(), // PROT_NONE
                MmapFlags::PRIVATE | MmapFlags::FIXED | MmapFlags::ANONYMOUS,
            )
            .expect("mmap PROT_NONE must succeed");

        // Verify mprotect can widen permissions on anonymous PROT_NONE mapping
        let res = vmar.mprotect(VmPerms::READ | VmPerms::WRITE, addr..addr + PAGE_SIZE);
        assert!(
            res.is_ok(),
            "mprotect on anonymous PROT_NONE reservation must succeed, got {:?}",
            res
        );
    }
}
