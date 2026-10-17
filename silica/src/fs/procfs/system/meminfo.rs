// SPDX-License-Identifier: GPL-2.0

//! `/proc/meminfo` generator.

use alloc::{string::String, vec::Vec};
use core::fmt::Write;

pub fn generate_meminfo() -> Vec<u8> {
    let boot = ostd::boot::boot_info();
    let mut total_ram: u64 = 0;
    for region in &boot.memory_regions {
        if region.typ().is_physical() {
            total_ram = total_ram.saturating_add(region.len() as u64);
        }
    }
    if total_ram == 0 {
        total_ram = 512 * 1024 * 1024;
    }

    let mut mapped_ram: u64 = 0;
    crate::proc::allproc_for_each(|p| {
        mapped_ram = mapped_ram.saturating_add(p.vmspace().total_mapped_size() as u64);
    });

    let free_ram = total_ram
        .saturating_sub(mapped_ram)
        .saturating_sub(32 * 1024 * 1024)
        .max(total_ram / 4);

    let total_kb = total_ram / 1024;
    let free_kb = free_ram / 1024;

    let mut s = String::new();
    let _ = writeln!(s, "MemTotal:       {:8} kB", total_kb);
    let _ = writeln!(s, "MemFree:        {:8} kB", free_kb);
    let _ = writeln!(s, "MemAvailable:   {:8} kB", free_kb);
    let _ = writeln!(s, "Buffers:               0 kB");
    let _ = writeln!(s, "Cached:            16384 kB");
    let _ = writeln!(s, "SwapTotal:             0 kB");
    let _ = writeln!(s, "SwapFree:              0 kB");
    s.into_bytes()
}
