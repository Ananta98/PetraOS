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

    /// Combines flags, e.g. `Gfp::KERNEL.with(Gfp::ATOMIC)` retains the
    /// non-blocking intent.
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// True when the caller prefers failing over waiting.
    pub fn is_nonblocking(self) -> bool {
        self.0 & NONBLOCKING != 0
    }
}

impl Default for Gfp {
    fn default() -> Self {
        Self::KERNEL
    }
}

/// Zero-fills a `len`-byte kernel buffer, mirroring `kzalloc(len, flags)`.
/// Returns `Some(buffer)` on success; `None` models Linux's `NULL` return.
pub fn kzalloc(len: usize, flags: Gfp) -> Option<Vec<u8>> {
    if len == 0 {
        return Some(Vec::new());
    }

    let mut buffer = Vec::new();
    match buffer.try_reserve_exact(len) {
        Ok(()) => {
            // The reservation covers the full request, so filling can no
            // longer reallocate.
            buffer.resize(len, 0);
            Some(buffer)
        }
        // `GFP_KERNEL` callers assume success; take the infallible path,
        // which aborts loudly on OOM just like the backing allocator.
        Err(_) if flags.is_nonblocking() => None,
        Err(_) => Some(alloc::vec![0u8; len]),
    }
}
