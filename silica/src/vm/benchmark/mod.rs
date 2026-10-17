// SPDX-License-Identifier: GPL-2.0

//! In-kernel Virtual Memory performance micro-benchmarking suite.
//!
//! Provides deterministic measurement of key VM operations using CPU cycle counters (`RDTSC`):
//! - Page fault latency (Zero-Page read vs private CoW write)
//! - Batched page population (`populate_range`)
//! - Fork address space duplication (`fork_from`)
//! - Non-allocating mapping query traversal (`iter_range`)

pub mod fault_bench;
pub mod fork_bench;
pub mod iter_bench;
pub mod populate_bench;

/// Reads the CPU time-stamp counter.
#[inline(always)]
pub fn read_tsc() -> u64 {
    ostd::arch::read_tsc()
}

/// Runs the complete VM micro-benchmark suite and logs cycle timings.
pub fn run_all_benchmarks() {
    ostd::early_println!("[VM BENCH] Starting Virtual Memory Micro-Benchmark Suite...");
    fault_bench::run();
    populate_bench::run();
    fork_bench::run();
    iter_bench::run();
    ostd::early_println!("[VM BENCH] All VM Micro-Benchmarks Completed.");
}

#[cfg(ktest)]
mod tests {
    use ostd::prelude::ktest;
    use super::*;

    #[ktest]
    fn run_vm_benchmark_suite_ktest() {
        run_all_benchmarks();
    }
}
