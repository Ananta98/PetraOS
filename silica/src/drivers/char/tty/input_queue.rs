// SPDX-License-Identifier: GPL-2.0

//! TTY Input Queue.
//!
//! Interrupt-safe ring buffer buffering keystrokes and raw bytes
//! before they are drained into the TTY line discipline.

use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

use ostd::sync::WaitQueue;

use crate::utils::ring_buffer::RingBuffer;

/// Bytes received from interrupt handlers, waiting to be fed to the line discipline.
static INPUT_QUEUE: RingBuffer<1024> = RingBuffer::new();

/// Serializes consumers of INPUT_QUEUE. Never taken from interrupt context.
static DRAIN_LOCK: Mutex<()> = Mutex::new(());

/// Set when `INPUT_QUEUE` holds bytes the input daemon has not drained yet.
static INPUT_PENDING: AtomicBool = AtomicBool::new(false);

/// Parks the input daemon until a keystroke is queued.
static INPUT_WAIT_QUEUE: WaitQueue = WaitQueue::new();

/// Queues an input byte. Lock-free; safe to call from interrupt handlers.
pub fn queue_input(ch: u8) {
    INPUT_QUEUE.push_overwrite(ch);
    INPUT_PENDING.store(true, Ordering::Release);
    INPUT_WAIT_QUEUE.wake_one();
    if let Some(console) = super::get_console() {
        console.wake_readers();
    }
}

/// Feeds all queued input bytes to the console line discipline. Thread context only.
pub fn drain_input() {
    let Some(_guard) = DRAIN_LOCK.try_lock() else {
        return;
    };
    let Some(console) = super::get_console() else {
        return;
    };
    while let Some(b) = INPUT_QUEUE.pop() {
        console.input_byte(b);
    }
}

/// Flushes any pending input in INPUT_QUEUE.
pub fn flush_input_queue() {
    let Some(_guard) = DRAIN_LOCK.try_lock() else {
        return;
    };
    while INPUT_QUEUE.pop().is_some() {}
}
