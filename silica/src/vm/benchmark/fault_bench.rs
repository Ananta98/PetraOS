// SPDX-License-Identifier: GPL-2.0

//! Page fault performance micro-benchmark.
//!
//! Compares latency of read fault (mapping global shared Zero-Page) vs.
//! write fault (breaking Zero-Page / CoW and allocating private frame).

use ostd::mm::PAGE_SIZE;

use crate::vm::{
    abi::{flags::MmapFlags, perms::VmPerms},
    benchmark::read_tsc,
    vmar::{Vmar, page_fault::PageFaultInfo},
};

/// Runs the page fault benchmark.
pub fn run() {
    let vmar = Vmar::new();
    let base = 0x6000_0000;
    let size = 10 * PAGE_SIZE;

    vmar.mmap_anonymous(
        base,
        size,
        VmPerms::READ | VmPerms::WRITE,
        MmapFlags::PRIVATE | MmapFlags::FIXED,
    )
    .expect("mmap for fault bench failed");

    // Measure Read Fault (Zero Page demand paging)
    let read_fault = PageFaultInfo::new(base, VmPerms::READ);
    let start_read = read_tsc();
    vmar.handle_page_fault(&read_fault)
        .expect("read fault handling failed");
    let elapsed_read = read_tsc().saturating_sub(start_read);

    // Measure Write Fault on the same page (CoW break)
    let write_fault = PageFaultInfo::new(base, VmPerms::WRITE);
    let start_write = read_tsc();
    vmar.handle_page_fault(&write_fault)
        .expect("write fault handling failed");
    let elapsed_write = read_tsc().saturating_sub(start_write);

    ostd::early_println!(
        "[VM BENCH] Fault: Zero-Page Read Fault: {} cycles | CoW Break Write Fault: {} cycles",
        elapsed_read,
        elapsed_write
    );
}

#[cfg(ktest)]
mod tests {
    use ostd::prelude::ktest;
    use super::*;

    #[ktest]
    fn bench_fault_latency() {
        run();
    }
}
