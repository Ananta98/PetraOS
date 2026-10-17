// SPDX-License-Identifier: GPL-2.0

//! Mapping query iteration performance micro-benchmark.
//!
//! Measures non-allocating range query traversal (`iter_range`) vs allocating query (`iter_in`).

use ostd::mm::PAGE_SIZE;

use crate::vm::{
    abi::{flags::MmapFlags, perms::VmPerms},
    benchmark::read_tsc,
    vmar::Vmar,
};

/// Runs the iteration benchmark across 64 mappings.
pub fn run() {
    let vmar = Vmar::new();
    let num_mappings = 64;

    for i in 0..num_mappings {
        let addr = 0x9000_0000 + (i * 2 * PAGE_SIZE);
        vmar.mmap_anonymous(
            addr,
            PAGE_SIZE,
            // Vary permissions to prevent merging
            if i % 2 == 0 {
                VmPerms::READ
            } else {
                VmPerms::READ | VmPerms::WRITE
            },
            MmapFlags::PRIVATE | MmapFlags::FIXED,
        )
        .expect("mmap for iter bench failed");
    }

    let search_range = 0x9000_0000..0x9000_0000 + (num_mappings * 2 * PAGE_SIZE);

    // 1. Non-allocating iter_range traversal
    let inner = vmar.inner.read();
    let start_zero_alloc = read_tsc();
    let mut count_zero_alloc = 0;
    for _ in inner.mappings.iter_range(&search_range) {
        count_zero_alloc += 1;
    }
    let elapsed_zero_alloc = read_tsc().saturating_sub(start_zero_alloc);

    // 2. Allocating iter_in traversal
    let start_alloc = read_tsc();
    let vec = inner.mappings.iter_in(&search_range);
    let count_alloc = vec.len();
    let elapsed_alloc = read_tsc().saturating_sub(start_alloc);
    drop(inner);

    assert_eq!(count_zero_alloc, num_mappings);
    assert_eq!(count_alloc, num_mappings);

    ostd::early_println!(
        "[VM BENCH] Iter: Non-allocating iter_range: {} cycles | Allocating iter_in: {} cycles ({}x faster)",
        elapsed_zero_alloc,
        elapsed_alloc,
        if elapsed_zero_alloc > 0 { elapsed_alloc / elapsed_zero_alloc } else { 1 }
    );
}

#[cfg(ktest)]
mod tests {
    use ostd::prelude::ktest;
    use super::*;

    #[ktest]
    fn bench_mapping_iter() {
        run();
    }
}
