//! Lock-free SPSC Ring Buffer
//!
//! A lock-free, wait-free Single-Producer Single-Consumer (SPSC) circular FIFO buffer
//! for storing bytes from hardware interrupts (keyboard, mouse, serial) without locks
//! or dynamic memory allocations.

use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

/// A lock-free, wait-free circular byte buffer.
pub struct ByteRingBuffer<const CAP: usize = 512> {
    buffer: [AtomicU8; CAP],
    head: AtomicUsize,
    tail: AtomicUsize,
}

impl<const CAP: usize> ByteRingBuffer<CAP> {
    pub const fn new() -> Self {
        Self {
            buffer: [const { AtomicU8::new(0) }; CAP],
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    /// Push a byte into the FIFO buffer (wait-free, producer context).
    /// Returns false if the buffer is full (dropping the byte to prevent overflow).
    pub fn push(&self, byte: u8) -> bool {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);

        if head.wrapping_sub(tail) >= CAP {
            return false;
        }

        let index = head % CAP;
        self.buffer[index].store(byte, Ordering::Relaxed);
        self.head.store(head.wrapping_add(1), Ordering::Release);
        true
    }

    /// Pop a byte from the FIFO buffer (wait-free, consumer context).
    pub fn pop(&self) -> Option<u8> {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);

        if head == tail {
            return None;
        }

        let index = tail % CAP;
        let byte = self.buffer[index].load(Ordering::Relaxed);
        self.tail.store(tail.wrapping_add(1), Ordering::Release);
        Some(byte)
    }

    /// Number of elements currently stored in the buffer.
    pub fn len(&self) -> usize {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Acquire);
        head.wrapping_sub(tail)
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.head.load(Ordering::Acquire) == self.tail.load(Ordering::Acquire)
    }

    /// Whether the buffer is full.
    pub fn is_full(&self) -> bool {
        self.len() >= CAP
    }

    /// Clear all elements in the buffer.
    pub fn clear(&self) {
        let head = self.head.load(Ordering::Acquire);
        self.tail.store(head, Ordering::Release);
    }
}

impl<const CAP: usize> Default for ByteRingBuffer<CAP> {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience aliases for keyboard, mouse, and general driver subsystems.
pub type RingBuffer<const CAP: usize = 512> = ByteRingBuffer<CAP>;
pub type KeyBuffer<const CAP: usize = 256> = ByteRingBuffer<CAP>;
pub type MouseRingBuffer<const CAP: usize = 512> = ByteRingBuffer<CAP>;
