// SPDX-License-Identifier: GPL-2.0

//! Address Space Layout Randomization (ASLR) Security Subsystem.
//!
//! Provides cryptographically secure entropy-derived virtual address randomization
//! for user and kernel address spaces to defend against Return-Oriented Programming (ROP)
//! and memory corruption exploits. Mirrors Linux `randomize_va_space` sysctl semantics.

use core::sync::atomic::{AtomicU32, Ordering};
use ostd::mm::PAGE_SIZE;

/// ASLR operation modes mirroring Linux `/proc/sys/kernel/randomize_va_space`.
pub const ASLR_DISABLED: u32 = 0;
pub const ASLR_CONSERVATIVE: u32 = 1;
pub const ASLR_FULL: u32 = 2;

/// Global ASLR enforcement level. Defaults to full randomization (mode 2).
static ASLR_LEVEL: AtomicU32 = AtomicU32::new(ASLR_FULL);

/// Returns the current global ASLR enforcement level.
pub fn get_aslr_level() -> u32 {
    ASLR_LEVEL.load(Ordering::Acquire)
}

/// Sets the global ASLR enforcement level.
pub fn set_aslr_level(level: u32) {
    ASLR_LEVEL.store(level.min(ASLR_FULL), Ordering::Release);
}

/// Returns true if ASLR is actively randomizing address spaces.
pub fn is_aslr_enabled() -> bool {
    get_aslr_level() != ASLR_DISABLED
}

/// Generates a randomized virtual memory allocation offset (up to 256 MiB, page-aligned)
/// using the kernel CSPRNG.
pub fn mmap_aslr_offset() -> usize {
    if !is_aslr_enabled() {
        return 0;
    }
    let mut rand_bytes = [0u8; 8];
    crate::crypto::get_random_bytes(&mut rand_bytes);
    let val = u64::from_le_bytes(rand_bytes) as usize;
    // 28 bits of entropy: up to 256 MiB (65536 pages)
    let pages = val % 65536;
    pages * PAGE_SIZE
}

/// Generates a randomized stack top offset (up to 1 MiB, 16-byte aligned for System V ABI)
/// using the kernel CSPRNG.
pub fn stack_aslr_offset() -> usize {
    if !is_aslr_enabled() {
        return 0;
    }
    let mut rand_bytes = [0u8; 8];
    crate::crypto::get_random_bytes(&mut rand_bytes);
    let val = u64::from_le_bytes(rand_bytes) as usize;
    // Randomize stack down by up to 1 MiB, keeping 16-byte alignment
    (val % (1024 * 1024)) & !0x0F
}

/// Generates a randomized heap (`brk`) base offset (up to 32 MiB, page-aligned).
pub fn brk_aslr_offset() -> usize {
    if get_aslr_level() < ASLR_FULL {
        return 0;
    }
    let mut rand_bytes = [0u8; 8];
    crate::crypto::get_random_bytes(&mut rand_bytes);
    let val = u64::from_le_bytes(rand_bytes) as usize;
    // Up to 32 MiB (8192 pages)
    let pages = val % 8192;
    pages * PAGE_SIZE
}
