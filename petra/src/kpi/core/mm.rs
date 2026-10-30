// SPDX-License-Identifier: GPL-2.0

//! GFP-flavored kernel memory allocation.
//!
//! Mirrors Linux `GFP_*` flags and `kzalloc()` as zero-backed kernel buffers,
//! returning `None` where Linux returns `NULL`. Narrowing: a safe-Rust
//! allocator cannot promise non-blocking behavior, so `GFP_ATOMIC` and
//! `GFP_NOWAIT` are honored as *may fail* (failing eagerly on reservation
//! errors) rather than as sleep-freedom guarantees; uninitialized
//! `kmalloc()` buffers are deliberately not provided at this layer yet.

use alloc::vec::Vec;

const NONBLOCKING: usize = 1 << 0;
const ZERO: usize = 1 << 1;

/// Linux `gfp_t`: allocation intent flags.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Gfp(usize);

impl Gfp {
    /// `GFP_KERNEL`: may wait to satisfy the request; only hard OOM fails.
    pub const KERNEL: Self = Self(0);

    /// `GFP_ATOMIC`: allocation from IRQ contexts; may fail immediately.
    pub const ATOMIC: Self = Self(NONBLOCKING);

    /// `GFP_NOWAIT`: like [`Gfp::ATOMIC`], without its IRQ-ready guarantee.
    pub const NOWAIT: Self = Self(NONBLOCKING);

    /// `__GFP_ZERO`: initialize memory with zeros.
    pub const ZERO: Self = Self(ZERO);

    /// Combines flags, e.g. `Gfp::KERNEL.with(Gfp::ATOMIC)` retains the
    /// non-blocking intent.
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// True when the caller prefers failing over waiting.
    pub fn is_nonblocking(self) -> bool {
        self.0 & NONBLOCKING != 0
    }

    /// True when zero initialization is requested.
    pub fn is_zero(self) -> bool {
        self.0 & ZERO != 0
    }
}

impl Default for Gfp {
    fn default() -> Self {
        Self::KERNEL
    }
}

/// Allocates a `len`-byte kernel buffer mirroring `kmalloc(len, flags)`.
pub fn kmalloc(len: usize, flags: Gfp) -> Option<Vec<u8>> {
    if len == 0 {
        return Some(Vec::new());
    }

    let mut buffer = Vec::new();
    match buffer.try_reserve_exact(len) {
        Ok(()) => {
            buffer.resize(len, 0);
            Some(buffer)
        }
        Err(_) if flags.is_nonblocking() => None,
        Err(_) => Some(alloc::vec![0u8; len]),
    }
}

/// Zero-fills a `len`-byte kernel buffer, mirroring `kzalloc(len, flags)`.
/// Returns `Some(buffer)` on success; `None` models Linux's `NULL` return.
pub fn kzalloc(len: usize, flags: Gfp) -> Option<Vec<u8>> {
    kmalloc(len, flags.with(Gfp::ZERO))
}

/// Allocates an array of `n` elements of `size` bytes each, zero-filled (`kcalloc`).
pub fn kcalloc(n: usize, size: usize, flags: Gfp) -> Option<Vec<u8>> {
    let total = n.checked_mul(size)?;
    kzalloc(total, flags)
}

/// Linux `vmalloc`: allocates virtually contiguous memory.
pub fn vmalloc(len: usize) -> Option<Vec<u8>> {
    kzalloc(len, Gfp::KERNEL)
}

/// Linux `vfree`: releases virtually contiguous memory.
pub fn vfree(_buf: Vec<u8>) {
    // Dropping Vec deallocates memory automatically
}
