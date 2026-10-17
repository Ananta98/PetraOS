// SPDX-License-Identifier: GPL-2.0

//! Lock-free Single-Producer Single-Consumer (SPSC) Ring Buffer.
//!
//! Provides a 100% safe Rust, lock-free, allocation-free, interrupt-safe circular queue
//! suitable for passing bytes or input events between ISRs/drivers and consumers.

use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

/// A 100% safe, lock-free Single-Producer Single-Consumer (SPSC) byte ring buffer.
pub struct RingBuffer<const CAP: usize> {
    buffer: [AtomicU8; CAP],
    head: AtomicUsize,
    tail: AtomicUsize,
}

impl<const CAP: usize> RingBuffer<CAP> {
    /// Creates a new, empty `RingBuffer`.
    pub const fn new() -> Self {
        assert!(CAP > 0, "RingBuffer capacity must be non-zero");

        Self {
            buffer: [const { AtomicU8::new(0) }; CAP],
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    /// Attempts to push a byte into the queue.
    ///
    /// Returns `Ok(())` on success, or `Err(byte)` if the queue is full.
    pub fn push(&self, val: u8) -> Result<(), u8> {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);

        if head.wrapping_sub(tail) >= CAP {
            return Err(val);
        }

        let idx = head % CAP;
        self.buffer[idx].store(val, Ordering::Relaxed);
        self.head.store(head.wrapping_add(1), Ordering::Release);
        Ok(())
    }

    /// Pushes a byte, overwriting the oldest byte if the queue is full.
    pub fn push_overwrite(&self, val: u8) {
        if self.push(val).is_err() {
            let _ = self.pop();
            let _ = self.push(val);
        }
    }

    /// Attempts to pop a byte from the queue.
    ///
    /// Returns `Some(byte)` if available, or `None` if empty.
    pub fn pop(&self) -> Option<u8> {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);

        if tail == head {
            return None;
        }

        let idx = tail % CAP;
        let val = self.buffer[idx].load(Ordering::Relaxed);
        self.tail.store(tail.wrapping_add(1), Ordering::Release);
        Some(val)
    }

    /// Returns the current number of elements in the queue.
    pub fn len(&self) -> usize {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Acquire);
        head.wrapping_sub(tail)
    }

    /// Returns `true` if the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns `true` if the queue is full.
    pub fn is_full(&self) -> bool {
        self.len() >= CAP
    }

    /// Returns the total capacity of the queue.
    pub const fn capacity(&self) -> usize {
        CAP
    }
}
