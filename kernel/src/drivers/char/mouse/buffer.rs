//! Lock-free SPSC Ring Buffer and WaitQueue for Mouse Events

use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use crate::sync::WaitQueue;

pub struct MouseRingBuffer<const CAP: usize = 512> {
    buffer: [AtomicU8; CAP],
    head: AtomicUsize,
    tail: AtomicUsize,
}

impl<const CAP: usize> MouseRingBuffer<CAP> {
    pub const fn new() -> Self {
        Self {
            buffer: [const { AtomicU8::new(0) }; CAP],
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    /// Push a byte into the ring buffer (wait-free, producer context).
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

    /// Pop a byte from the ring buffer (wait-free, consumer context).
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

    /// Number of bytes currently queued.
    pub fn len(&self) -> usize {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Acquire);
        head.wrapping_sub(tail)
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.head.load(Ordering::Acquire) == self.tail.load(Ordering::Acquire)
    }

    /// Clear all queued bytes.
    pub fn clear(&self) {
        let head = self.head.load(Ordering::Acquire);
        self.tail.store(head, Ordering::Release);
    }
}

impl<const CAP: usize> Default for MouseRingBuffer<CAP> {
    fn default() -> Self {
        Self::new()
    }
}

/// Global mouse raw byte ring buffer for `/dev/input/mice` and `/dev/psaux`.
pub static MOUSE_RING_BUFFER: MouseRingBuffer<512> = MouseRingBuffer::new();

/// Wait queue for blocking reads on mouse device.
pub static MOUSE_WAIT_QUEUE: WaitQueue = WaitQueue::new();
