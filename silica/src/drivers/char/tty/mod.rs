// SPDX-License-Identifier: GPL-2.0

pub mod console;
pub mod ldisc;

use alloc::sync::Arc;
use spin::{Mutex, Once};

use crate::drivers::register_chrdev;
use console::Console;

static CONSOLE: Once<Arc<Console>> = Once::new();

/// Initialize the TTY subsystem
pub fn init() -> crate::api::errno::Result<()> {
    let console_dev = Arc::new(Console::new());
    let _ = CONSOLE.call_once(|| Arc::clone(&console_dev));
    register_chrdev(console_dev);
    ostd::info!("TTY subsystem initialized");
    Ok(())
}

/// Returns the primary system console device.
pub fn get_console() -> Option<Arc<Console>> {
    CONSOLE.get().cloned()
}

/// Bytes received from interrupt handlers, waiting to be fed to the line discipline.
///
/// Interrupt handlers must never take the line-discipline / console locks (the
/// interrupted thread may already hold them, which would spin forever), so they
/// only push here. The queue is drained from thread context by [`drain_input`].
static INPUT_QUEUE: crate::utils::lockless_queue::LocklessQueue<1024> =
    crate::utils::lockless_queue::LocklessQueue::new();

/// Serialises consumers of [`INPUT_QUEUE`]. Never taken from interrupt context.
static DRAIN_LOCK: Mutex<()> = Mutex::new(());

/// Queues an input byte. Lock-free; safe to call from an interrupt handler.
pub fn queue_input(ch: u8) {
    INPUT_QUEUE.push_overwrite(ch);
    if let Some(console) = CONSOLE.get() {
        console.notify_input();
    }
}

/// Feeds all queued input bytes to the line discipline. Thread context only.
pub fn drain_input() {
    // If another thread is already draining, it will deliver the bytes.
    let Some(_guard) = DRAIN_LOCK.try_lock() else {
        return;
    };
    while let Some(b) = INPUT_QUEUE.pop() {
        handle_keyboard_char(b);
    }
}

/// Dispatches an incoming character from the keyboard into the TTY line discipline.
pub fn handle_keyboard_char(ch: u8) {
    if let Some(console) = CONSOLE.get() {
        console.handle_input(ch);
    }
}

crate::module!(
    "TTY Console Subsystem",
    "Ananta98",
    crate::modules::InitcallLevel::Device,
    init
);
