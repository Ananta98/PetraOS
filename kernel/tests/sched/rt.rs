// ── Module shim: expose crate::sched as the test binary's crate path ─────────

#[path = "."]
pub mod sched {
    #[path = "../../src/sched/sched_thread.rs"]
    pub mod sched_thread;

    #[path = "../../src/sched/rt.rs"]
    pub mod rt;
}

use sched::sched_thread::{SchedThread, ThreadId};
use sched::rt::RtRunQueue;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn fifo(id: u64, prio: u32) -> SchedThread {
    SchedThread::new_fifo(ThreadId(id), prio)
}

fn rr_with_slice(id: u64, prio: u32, slice_ns: u64) -> SchedThread {
    SchedThread::new_rr_with_slice(ThreadId(id), prio, slice_ns)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Higher RT priority is always picked first, regardless of insertion order.
#[test]
fn test_rt_priority_order() {
    let mut rq = RtRunQueue::new();

    rq.enqueue(fifo(1, 10));
    rq.enqueue(fifo(2, 50));
    rq.enqueue(fifo(3, 1));

    let a = rq.dequeue_next().expect("thread");
    let b = rq.dequeue_next().expect("thread");
    let c = rq.dequeue_next().expect("thread");

    assert_eq!(a.id, ThreadId(2), "priority 50 must run first");
    assert_eq!(b.id, ThreadId(1), "priority 10 second");
    assert_eq!(c.id, ThreadId(3), "priority 1 last");

    assert!(rq.is_empty());
}

/// Threads at the same priority follow FIFO ordering.
#[test]
fn test_rt_fifo_same_priority_order() {
    let mut rq = RtRunQueue::new();

    rq.enqueue(fifo(10, 20));
    rq.enqueue(fifo(11, 20));
    rq.enqueue(fifo(12, 20));

    let a = rq.dequeue_next().unwrap();
    let b = rq.dequeue_next().unwrap();
    let c = rq.dequeue_next().unwrap();

    assert_eq!(a.id, ThreadId(10), "first in → first out at same priority");
    assert_eq!(b.id, ThreadId(11));
    assert_eq!(c.id, ThreadId(12));
}

/// A RR thread whose slice expires is rotated to the back of its priority level.
#[test]
fn test_rt_rr_tick_rotates_on_slice_expiry() {
    let mut rq = RtRunQueue::new();
    let slice_ns = 5_000_000u64; // 5 ms

    rq.enqueue(rr_with_slice(1, 30, slice_ns));
    rq.enqueue(rr_with_slice(2, 30, slice_ns));

    // thread 1 is at the front; consume its entire slice.
    let rotated = rq.tick(ThreadId(1), slice_ns);
    assert!(rotated, "tick must find the thread");

    // After rotation, thread 2 should now be at the front.
    let front = rq.pick_next().expect("should have threads");
    assert_eq!(front.id, ThreadId(2), "thread 1 should have rotated behind thread 2");
}

/// RR tick that does NOT exhaust the slice should NOT rotate the thread.
#[test]
fn test_rt_rr_tick_partial_slice_no_rotation() {
    let mut rq = RtRunQueue::new();
    let slice_ns = 10_000_000u64; // 10 ms

    rq.enqueue(rr_with_slice(1, 30, slice_ns));
    rq.enqueue(rr_with_slice(2, 30, slice_ns));

    // Consume only half the slice.
    rq.tick(ThreadId(1), slice_ns / 2);

    // Thread 1 should still be at the front.
    let front = rq.pick_next().unwrap();
    assert_eq!(front.id, ThreadId(1), "thread 1 should remain at front");
    assert_eq!(front.remaining_slice, slice_ns / 2, "remaining slice must decrease");
}

/// FIFO threads are not affected by tick (no slice tracking).
#[test]
fn test_rt_fifo_tick_noop() {
    let mut rq = RtRunQueue::new();

    rq.enqueue(fifo(1, 40));
    rq.enqueue(fifo(2, 40));

    // Tick a FIFO thread with an enormous delta — it should not rotate.
    rq.tick(ThreadId(1), u64::MAX);

    let front = rq.pick_next().unwrap();
    assert_eq!(front.id, ThreadId(1), "FIFO thread must not be rotated by tick");
}

/// Empty queue returns None for dequeue_next and pick_next.
#[test]
fn test_rt_empty_queue_returns_none() {
    let mut rq = RtRunQueue::new();
    assert!(rq.dequeue_next().is_none());
    assert!(rq.pick_next().is_none());
    assert_eq!(rq.len(), 0);
}

/// Removing a thread by id decrements the count correctly.
#[test]
fn test_rt_remove_by_id() {
    let mut rq = RtRunQueue::new();

    rq.enqueue(fifo(1, 10));
    rq.enqueue(fifo(2, 20));

    let removed = rq.remove(ThreadId(1)).expect("thread 1 should be removable");
    assert_eq!(removed.id, ThreadId(1));
    assert_eq!(rq.len(), 1);

    let next = rq.dequeue_next().unwrap();
    assert_eq!(next.id, ThreadId(2));
}

/// Removing a non-existent thread returns None without changing count.
#[test]
fn test_rt_remove_nonexistent_returns_none() {
    let mut rq = RtRunQueue::new();
    rq.enqueue(fifo(1, 10));

    assert!(rq.remove(ThreadId(999)).is_none());
    assert_eq!(rq.len(), 1);
}

/// RT priority values are clamped to [1, 99].
#[test]
fn test_rt_priority_clamping() {
    let low = fifo(1, 0);    // should clamp to 1
    let high = fifo(2, 200); // should clamp to 99
    assert_eq!(low.priority, 1);
    assert_eq!(high.priority, 99);
}

/// After slice expiry and reset, remaining_slice is restored to time_slice_ns.
#[test]
fn test_rt_rr_slice_reset_after_rotation() {
    let mut rq = RtRunQueue::new();
    let slice_ns = 3_000_000u64;

    rq.enqueue(rr_with_slice(1, 10, slice_ns));

    // Expire the slice — thread rotates and gets a fresh slice.
    rq.tick(ThreadId(1), slice_ns);

    let t = rq.pick_next().unwrap();
    assert_eq!(t.remaining_slice, slice_ns, "slice must be fully reset after rotation");
}
