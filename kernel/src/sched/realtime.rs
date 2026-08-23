//! Lockless Real-Time (RT) Run Queue with O(1) priority selection.
//!
//! Provides a Multi-Producer Single-Consumer (MPSC) lock-free priority queue
//! supporting 100 distinct real-time priority levels (0..=99).
//!
//! Architecture:
//! 1. Atomic Bitmap: 2 x `AtomicU64` bitmasks tracking non-empty priority levels.
//!    Highest priority selection is O(1) using CPU intrinsic `leading_zeros` (`lzcnt`/`bsr`).
//! 2. Ingress Stack (MPSC): Each priority level has an `AtomicPtr<RtNode>` Treiber stack
//!    where any CPU or interrupt handler can locklessly enqueue a thread using CAS.
//! 3. Egress Queue (SPSC): The scheduler consumer on the local CPU drains the ingress stack,
//!    reverses it to maintain strict FIFO order, and pops the next runnable thread.

use super::policy::{RtPriority, RT_PRIO_COUNT};
use crate::proc::thread::{Thread, ThreadId};
use crate::sync::spinlock::Spinlock;
use alloc::boxed::Box;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicPtr, AtomicU64, Ordering};

/// A node in the lockless real-time run queue.
#[repr(C)]
pub struct RtNode {
    /// Next pointer in the singly-linked list.
    pub next: *mut RtNode,
    /// Atomic next pointer for MPSC ingress stack.
    pub atomic_next: AtomicPtr<RtNode>,
    /// Thread ID for quick identification.
    pub tid: ThreadId,
    /// Real-time priority assigned to this thread.
    pub priority: u8,
    /// Reference to the thread.
    pub thread: Arc<Spinlock<Thread>>,
}

impl RtNode {
    /// Creates a new `RtNode` wrapping a thread.
    pub fn new(thread: Arc<Spinlock<Thread>>, priority: u8, tid: ThreadId) -> Self {
        Self {
            next: core::ptr::null_mut(),
            atomic_next: AtomicPtr::new(core::ptr::null_mut()),
            tid,
            priority,
            thread,
        }
    }
}

/// Lockless Real-Time Run Queue.
pub struct RtRunQueue {
    /// Bitmaps tracking non-empty priority levels.
    /// `bitmap[0]`: priorities 0..63
    /// `bitmap[1]`: priorities 64..99
    bitmap: [AtomicU64; 2],

    /// Ingress Treiber stacks for multi-producer push (one per priority level).
    ingress: [AtomicPtr<RtNode>; RT_PRIO_COUNT],

    /// Egress FIFO heads for single-consumer pop (one per priority level).
    egress: [AtomicPtr<RtNode>; RT_PRIO_COUNT],
}

// SAFETY: All fields are either AtomicPtr or AtomicU64, safe to share across threads/CPUs.
unsafe impl Send for RtRunQueue {}
unsafe impl Sync for RtRunQueue {}

impl RtRunQueue {
    /// Creates a new, empty `RtRunQueue`.
    pub const fn new() -> Self {
        const NULL_PTR: AtomicPtr<RtNode> = AtomicPtr::new(core::ptr::null_mut());
        Self {
            bitmap: [AtomicU64::new(0), AtomicU64::new(0)],
            ingress: [NULL_PTR; RT_PRIO_COUNT],
            egress: [NULL_PTR; RT_PRIO_COUNT],
        }
    }

    /// Sets the bitmap bit for the specified priority level.
    #[inline]
    fn set_bit(&self, prio: u8) {
        let (word, bit) = if prio < 64 {
            (0, prio)
        } else {
            (1, prio - 64)
        };
        self.bitmap[word].fetch_or(1 << bit, Ordering::Release);
    }

    /// Clears the bitmap bit for the specified priority level.
    #[inline]
    fn clear_bit(&self, prio: u8) {
        let (word, bit) = if prio < 64 {
            (0, prio)
        } else {
            (1, prio - 64)
        };
        self.bitmap[word].fetch_and(!(1 << bit), Ordering::Release);
    }

    /// Finds the highest non-empty priority level in O(1) time.
    /// Returns `None` if the queue is completely empty.
    pub fn find_highest_priority(&self) -> Option<u8> {
        // Check priorities 64..99 first (higher numerical value = higher priority)
        let high_word = self.bitmap[1].load(Ordering::Acquire);
        if high_word != 0 {
            let leading = high_word.leading_zeros() as u8;
            let bit = 63 - leading;
            return Some(64 + bit);
        }

        // Check priorities 0..63
        let low_word = self.bitmap[0].load(Ordering::Acquire);
        if low_word != 0 {
            let leading = low_word.leading_zeros() as u8;
            let bit = 63 - leading;
            return Some(bit);
        }

        None
    }

    /// Enqueues a real-time thread into the run queue locklessly.
    ///
    /// This method is wait-free / lock-free and safe to call from interrupt handlers
    /// and any CPU core.
    pub fn enqueue(&self, thread: Arc<Spinlock<Thread>>, priority: RtPriority) {
        let prio = priority.value();
        let tid = thread.lock().tid;

        let node = Box::new(RtNode::new(thread, prio, tid));
        let node_ptr = Box::into_raw(node);

        // Lock-free MPSC push onto ingress stack
        loop {
            let head = self.ingress[prio as usize].load(Ordering::Acquire);
            // SAFETY: node_ptr is uniquely owned until successful CAS.
            unsafe {
                (*node_ptr).atomic_next.store(head, Ordering::Relaxed);
            }

            if self.ingress[prio as usize]
                .compare_exchange_weak(
                    head,
                    node_ptr,
                    Ordering::Release,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                break;
            }
        }

        // Mark the priority active in the bitmap
        self.set_bit(prio);
    }

    /// Pops the next highest-priority real-time thread (single-consumer).
    ///
    /// Returns `None` if there are no runnable real-time threads.
    pub fn dequeue_highest(&self) -> Option<Arc<Spinlock<Thread>>> {
        while let Some(prio) = self.find_highest_priority() {
            let idx = prio as usize;

            // 1. Check if we have nodes in the egress queue
            let mut egress_head = self.egress[idx].load(Ordering::Acquire);

            // 2. If egress queue is empty, transfer all nodes from ingress stack
            if egress_head.is_null() {
                let ingress_head = self.ingress[idx].swap(core::ptr::null_mut(), Ordering::AcqRel);
                if ingress_head.is_null() {
                    // Both ingress and egress are empty for this priority, clear bit and continue
                    self.clear_bit(prio);
                    continue;
                }

                // Reverse the LIFO ingress stack into FIFO order for egress
                let mut current = ingress_head;
                let mut prev: *mut RtNode = core::ptr::null_mut();

                while !current.is_null() {
                    // SAFETY: We have exclusive access to these nodes transferred from ingress.
                    unsafe {
                        let next = (*current).atomic_next.load(Ordering::Relaxed);
                        (*current).next = prev;
                        prev = current;
                        current = next;
                    }
                }

                egress_head = prev;
            }

            // 3. Pop the front node from egress
            if !egress_head.is_null() {
                // SAFETY: egress_head is a valid non-null pointer owned by the consumer.
                let (next_head, thread) = unsafe {
                    let next = (*egress_head).next;
                    let thread = (*egress_head).thread.clone();
                    // Reclaim Box memory for the dequeued node
                    drop(Box::from_raw(egress_head));
                    (next, thread)
                };

                self.egress[idx].store(next_head, Ordering::Release);

                // If egress became empty and ingress is also empty, clear the bitmap bit
                if next_head.is_null() && self.ingress[idx].load(Ordering::Acquire).is_null() {
                    self.clear_bit(prio);
                }

                return Some(thread);
            }
        }

        None
    }

    /// Checks whether the real-time run queue has any runnable threads.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.find_highest_priority().is_none()
    }
}
