// SPDX-License-Identifier: GPL-2.0

//! Fork address space duplication performance micro-benchmark.
//!
//! Measures cycles to duplicate a complex address space with multiple mappings.

use ostd::mm::PAGE_SIZE;

use crate::vm::{
    abi::{flags::MmapFlags, perms::VmPerms},
    benchmark::read_tsc,
    vmar::Vmar,
};

/// Runs the fork duplication benchmark across 32 distinct mappings.
pub fn run() {
    let parent = Vmar::new();
    let num_mappings = 32;

    for i in 0..num_mappings {
        let addr = 0x8000_0000 + (i * 4 * PAGE_SIZE);
        parent
            .mmap_anonymous(
                addr,
                2 * PAGE_SIZE,
                VmPerms::READ | VmPerms::WRITE,
                MmapFlags::PRIVATE | MmapFlags::FIXED,
            )
            .expect("parent mmap failed");

        // Touch the first page so it is resident
        parent
            .populate_range(&(addr..addr + PAGE_SIZE))
            .expect("populate failed");
    }

    let start = read_tsc();
    let child = Vmar::fork_from(&parent);
    let elapsed = read_tsc().saturating_sub(start);

    ostd::early_println!(
        "[VM BENCH] Fork: Duplicated {} mappings (with resident pages) in {} cycles",
        num_mappings,
        elapsed
    );

    // Verify child has the mappings
    assert_eq!(child.total_mapped_size(), parent.total_mapped_size());
}

#[cfg(ktest)]
mod tests {
    use ostd::prelude::ktest;
    use super::*;

    #[ktest]
    fn bench_fork_duplication() {
        run();
    }
}
