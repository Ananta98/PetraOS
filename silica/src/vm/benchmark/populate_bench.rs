// SPDX-License-Identifier: GPL-2.0

//! Batched page population (`populate_range`) performance micro-benchmark.
//!
//! Measures cycles to pre-fault a contiguous range of pages using batched TLB invalidation.

use ostd::mm::PAGE_SIZE;

use crate::vm::{
    abi::{flags::MmapFlags, perms::VmPerms},
    benchmark::read_tsc,
    vmar::Vmar,
};

/// Runs the populate benchmark across 256 pages (1 MiB).
pub fn run() {
    let vmar = Vmar::new();
    let pages = 256;
    let base = 0x7000_0000;
    let total_size = pages * PAGE_SIZE;

    vmar.mmap_anonymous(
        base,
        total_size,
        VmPerms::READ | VmPerms::WRITE,
        MmapFlags::PRIVATE | MmapFlags::FIXED,
    )
    .expect("mmap for populate bench failed");

    let range = base..base + total_size;
    let start = read_tsc();
    vmar.populate_range(&range)
        .expect("populate_range failed in bench");
    let elapsed = read_tsc().saturating_sub(start);
    let cycles_per_page = elapsed / (pages as u64);

    ostd::early_println!(
        "[VM BENCH] Populate: {} pages ({} KiB) in {} cycles ({} cycles/page)",
        pages,
        total_size / 1024,
        elapsed,
        cycles_per_page
    );
}

#[cfg(ktest)]
mod tests {
    use ostd::prelude::ktest;
    use super::*;

    #[ktest]
    fn bench_populate_range() {
        run();
    }
}
