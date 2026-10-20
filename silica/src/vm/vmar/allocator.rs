// SPDX-License-Identifier: GPL-2.0

//! Virtual address range allocation and free region searching.

use core::ops::Range;
use ostd::mm::{PAGE_SIZE, Vaddr};

use super::{VMAR_LOWEST_ADDR, mappings::Mappings};
use crate::api::errno::Errno;

/// The address space has no room for a mapping of the requested size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoRoom;

impl From<NoRoom> for Errno {
    fn from(_: NoRoom) -> Errno {
        Errno::ENOMEM
    }
}

/// Returns a region of `size` bytes inside `hole`, aligned down from its
/// end so that it sits as high in the hole as `align` allows.
fn take_from_hole(hole: &Range<Vaddr>, size: usize, align: usize) -> Option<Range<Vaddr>> {
    let start = hole
        .end
        .checked_sub(size)?
        .checked_div(align)?
        .checked_mul(align)?;
    (start >= hole.start).then_some(start..start + size)
}

/// Returns a free region of `size` bytes below `high_limit`, searching downwards
/// from high addresses to low addresses.
pub fn find_free_region(
    mappings: &Mappings,
    size: usize,
    high_limit: Vaddr,
) -> Result<Range<Vaddr>, NoRoom> {
    let low_limit = VMAR_LOWEST_ADDR;
    if high_limit <= low_limit || size > high_limit - low_limit {
        return Err(NoRoom);
    }

    let mut ceiling = high_limit;
    for mapping in mappings.iter().rev() {
        // Skip mappings that lie entirely above our search ceiling
        if mapping.start() >= ceiling {
            continue;
        }

        let hole_end = ceiling;
        let hole_start = mapping.end().max(low_limit);
        if hole_start < hole_end {
            if let Some(region) = take_from_hole(&(hole_start..hole_end), size, PAGE_SIZE) {
                return Ok(region);
            }
        }
        if mapping.start() <= low_limit {
            return Err(NoRoom);
        }
        ceiling = mapping.start().min(ceiling);
    }

    if low_limit < ceiling {
        take_from_hole(&(low_limit..ceiling), size, PAGE_SIZE).ok_or(NoRoom)
    } else {
        Err(NoRoom)
    }
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use crate::vm::abi::{flags::MmapFlags, perms::VmPerms};
    use crate::vm::vmar::Vmar;
    use ostd::prelude::ktest;

    #[ktest]
    fn repro_find_free_region_with_mapping_above_ceiling() {
        let vmar = Vmar::new();
        let high = vmar.inner.read().default_high_limit();

        // Place a fixed mapping at high_limit
        vmar.mmap_anonymous(
            high,
            PAGE_SIZE,
            VmPerms::READ | VmPerms::WRITE,
            MmapFlags::PRIVATE | MmapFlags::FIXED | MmapFlags::ANONYMOUS,
        )
        .expect("fixed map at high limit");

        let inner = vmar.inner.read();
        let res = find_free_region(&inner.mappings, PAGE_SIZE, high);
        assert!(
            res.is_ok(),
            "find_free_region must succeed below high limit: {:?}",
            res
        );
        assert!(
            res.unwrap().end <= high,
            "region must not exceed high_limit"
        );
    }
}
