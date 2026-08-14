//! Key and Character Ring Buffer
//!
//! A thread-safe circular FIFO buffer for storing received characters and key events
//! from keyboard interrupts without dynamic memory allocations.

use crate::sync::spinlock::Spinlock;

pub struct KeyBuffer<const CAP: usize = 256> {
    buffer: [u8; CAP],
    head: usize,
    tail: usize,
    count: usize,
}

impl<const CAP: usize> KeyBuffer<CAP> {
    pub const fn new() -> Self {
        Self {
            buffer: [0; CAP],
            head: 0,
            tail: 0,
            count: 0,
        }
    }

    /// Push a byte into the FIFO buffer.
    /// If full, overwrites the oldest element to make room.
    pub fn push(&mut self, byte: u8) -> bool {
        if self.count == CAP {
            // Buffer is full: overwrite oldest (tail)
            self.tail = (self.tail + 1) % CAP;
            self.count -= 1;
        }

        self.buffer[self.head] = byte;
        self.head = (self.head + 1) % CAP;
        self.count += 1;
        true
    }

    /// Pop a byte from the FIFO buffer.
    pub fn pop(&mut self) -> Option<u8> {
        if self.count == 0 {
            return None;
        }

        let byte = self.buffer[self.tail];
        self.tail = (self.tail + 1) % CAP;
        self.count -= 1;
        Some(byte)
    }

    /// Number of elements currently stored in the buffer.
    pub fn len(&self) -> usize {
        self.count
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Clear all elements in the buffer.
    pub fn clear(&mut self) {
        self.head = 0;
        self.tail = 0;
        self.count = 0;
    }
}

pub static KEY_RING_BUFFER: Spinlock<KeyBuffer<256>> = Spinlock::new(KeyBuffer::new());
