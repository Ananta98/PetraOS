// SPDX-License-Identifier: GPL-2.0

//! Lightweight serial performance measurement helpers.
//!
//! Captures TSC cycles, monotonic nanoseconds, frame time, FPS, and RAM usage
//! and emits structured `PERF:` lines over the kernel serial console.

use core::sync::atomic::{AtomicUsize, Ordering};

/// Total bytes currently held by active DRM GEM objects.
static GEM_BYTES_ALLOCATED: AtomicUsize = AtomicUsize::new(0);
/// Total bytes currently held by the VT shadow framebuffer.
static SHADOW_FB_BYTES: AtomicUsize = AtomicUsize::new(0);

/// Records GEM allocation/deallocation delta in bytes.
pub fn add_gem_bytes(delta: usize) {
    GEM_BYTES_ALLOCATED.fetch_add(delta, Ordering::Relaxed);
}

/// Records GEM deallocation in bytes.
pub fn sub_gem_bytes(delta: usize) {
    GEM_BYTES_ALLOCATED.fetch_sub(delta, Ordering::Relaxed);
}

/// Returns total bytes currently allocated by GEM objects.
pub fn gem_bytes() -> usize {
    GEM_BYTES_ALLOCATED.load(Ordering::Relaxed)
}

/// Sets the active VT shadow framebuffer size in bytes.
pub fn set_shadow_fb_bytes(bytes: usize) {
    SHADOW_FB_BYTES.store(bytes, Ordering::Relaxed);
}

/// Returns the active VT shadow framebuffer size in bytes.
pub fn shadow_fb_bytes() -> usize {
    SHADOW_FB_BYTES.load(Ordering::Relaxed)
}

/// Returns the current hardware timestamp counter.
#[inline(always)]
pub fn read_cycles() -> u64 {
    ostd::arch::read_tsc()
}

/// Returns current monotonic time in nanoseconds.
#[inline(always)]
pub fn now_ns() -> u64 {
    crate::time::get_monotonic_ns()
}

/// Computes `(elapsed_cycles, elapsed_us)` from start snapshots.
#[inline(always)]
pub fn elapsed(start_cycles: u64, start_ns: u64) -> (u64, u64) {
    let cycles = read_cycles().saturating_sub(start_cycles);
    let us = now_ns().saturating_sub(start_ns) / 1_000;
    (cycles, us)
}

/// Computes effective FPS from a frame duration in microseconds.
#[inline(always)]
pub fn fps_from_us(us: u64) -> u64 {
    if us == 0 {
        0
    } else {
        1_000_000 / us
    }
}

/// Returns total physical RAM in bytes reported by the bootloader memory map.
pub fn total_physical_ram_bytes() -> u64 {
    let boot = ostd::boot::boot_info();
    let mut total: u64 = 0;
    for region in &boot.memory_regions {
        if region.typ().is_physical() {
            total = total.saturating_add(region.len() as u64);
        }
    }
    total
}

/// Prints a summary of physical RAM and graphics/network memory footprints over serial.
pub fn log_memory_summary(tag: &str) {
    let total_bytes = total_physical_ram_bytes();
    let total_mib = total_bytes / (1024 * 1024);
    let shadow = shadow_fb_bytes();
    let gem = gem_bytes();
    let fb_bytes = crate::drivers::drm::framebuffer::get_framebuffer()
        .map(|fb| fb.total_size)
        .unwrap_or(0);
    ostd::info!(
        "PERF_MEM[{}]: total_ram={} bytes ({} MiB), fb_mmio={} bytes, shadow_fb={} bytes, gem={} bytes",
        tag,
        total_bytes,
        total_mib,
        fb_bytes,
        shadow,
        gem
    );
}
