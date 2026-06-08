// ── Module shim: expose crate::sched as the test binary's crate path ─────────
//
// The source files use `crate::sched::sched_thread`, `crate::sched::cfs`, etc.
// We recreate that exact module hierarchy here so the imports resolve
// identically to how they would inside the kernel binary.

#[path = "."]
pub mod sched {
    #[path = "../../src/sched/sched_thread.rs"]
    pub mod sched_thread;

    #[path = "../../src/sched/cfs.rs"]
    pub mod cfs;
}

use sched::sched_thread::{SchedThread, ThreadId};
use sched::cfs::CfsRunQueue;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn task(id: u64, vruntime: u64) -> SchedThread {
    let mut t = SchedThread::new_normal(ThreadId(id));
    t.vruntime = vruntime;
    t
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Enqueue three threads with distinct vruntimes; verify dequeue is min-first.
#[test]
fn test_cfs_enqueue_dequeue_order() {
    let mut rq = CfsRunQueue::new();

    rq.enqueue(task(1, 300));
    rq.enqueue(task(2, 100));
    rq.enqueue(task(3, 200));

    let a = rq.dequeue_min().expect("should have thread");
    let b = rq.dequeue_min().expect("should have thread");
    let c = rq.dequeue_min().expect("should have thread");

    assert_eq!(a.id, ThreadId(2), "min vruntime (100) should dequeue first");
    assert_eq!(b.id, ThreadId(3), "next min vruntime (200) should be second");
    assert_eq!(c.id, ThreadId(1), "largest vruntime (300) should be last");

    assert!(rq.is_empty());
}

/// pick_next returns the minimum without removing the thread.
#[test]
fn test_cfs_pick_next_does_not_remove() {
    let mut rq = CfsRunQueue::new();

    rq.enqueue(task(10, 50));
    rq.enqueue(task(20, 25));

    let peeked_id = rq.pick_next().expect("should peek").id;
    assert_eq!(peeked_id, ThreadId(20));
    assert_eq!(rq.len(), 2, "pick_next must not remove the thread");
}

/// After updating vruntime the thread moves to its new sorted position.
#[test]
fn test_cfs_vruntime_update_changes_order() {
    let mut rq = CfsRunQueue::new();

    rq.enqueue(task(1, 0));
    rq.enqueue(task(2, 500));

    // Thread 1 has vruntime = 0, thread 2 = 500.  Advance thread 1 by a big delta
    // so that it overtakes thread 2.
    // weight = NICE_0_WEIGHT (1024), delta = 600 ns
    // vruntime_delta = 600 * 1024 / 1024 = 600  → new vruntime = 600
    let delta_ns = 600u64;
    let updated = rq.update_vruntime(ThreadId(1), delta_ns);
    assert!(updated, "thread must be found for vruntime update");

    // Now thread 2 (500) < thread 1 (600), so thread 2 should dequeue first.
    let first = rq.dequeue_min().expect("thread");
    assert_eq!(first.id, ThreadId(2));
    let second = rq.dequeue_min().expect("thread");
    assert_eq!(second.id, ThreadId(1));
}

/// New threads are lifted to min_vruntime so they don't jump ahead of existing threads.
#[test]
fn test_cfs_new_task_lifted_to_min_vruntime() {
    let mut rq = CfsRunQueue::new();

    // Enqueue a thread at vruntime 1000, then dequeue it → min_vruntime advances to 1000.
    let mut t = task(1, 0);
    t.vruntime = 1000;
    rq.enqueue(t);
    let _ = rq.dequeue_min(); // min_vruntime becomes 1000

    // A brand-new thread with vruntime = 0 should be lifted to 1000.
    rq.enqueue(task(2, 0));
    let peeked = rq.pick_next().expect("should have thread");
    assert_eq!(
        peeked.vruntime, 1000,
        "new thread vruntime must be lifted to min_vruntime"
    );
}

/// Empty queue returns None for both pick_next and dequeue_min.
#[test]
fn test_cfs_empty_queue_returns_none() {
    let mut rq = CfsRunQueue::new();

    assert!(rq.pick_next().is_none());
    assert!(rq.dequeue_min().is_none());
    assert_eq!(rq.len(), 0);
}

/// Removing a thread by id works even when it is not the minimum.
#[test]
fn test_cfs_remove_by_id() {
    let mut rq = CfsRunQueue::new();

    rq.enqueue(task(1, 100));
    rq.enqueue(task(2, 200));
    rq.enqueue(task(3, 300));

    let removed = rq.remove(ThreadId(2)).expect("thread 2 should be removable");
    assert_eq!(removed.id, ThreadId(2));
    assert_eq!(rq.len(), 2);

    // Remaining threads should still be in order.
    let a = rq.dequeue_min().unwrap();
    let b = rq.dequeue_min().unwrap();
    assert_eq!(a.id, ThreadId(1));
    assert_eq!(b.id, ThreadId(3));
}

/// Removing a thread that does not exist returns None and leaves the queue intact.
#[test]
fn test_cfs_remove_nonexistent_returns_none() {
    let mut rq = CfsRunQueue::new();
    rq.enqueue(task(1, 0));

    let result = rq.remove(ThreadId(999));
    assert!(result.is_none());
    assert_eq!(rq.len(), 1);
}

/// update_vruntime on a non-existent thread returns false.
#[test]
fn test_cfs_update_vruntime_nonexistent() {
    let mut rq = CfsRunQueue::new();
    assert!(!rq.update_vruntime(ThreadId(42), 1000));
}

/// Saturating arithmetic prevents vruntime from overflowing.
#[test]
fn test_cfs_vruntime_saturates_on_overflow() {
    let mut rq = CfsRunQueue::new();

    let mut t = task(1, u64::MAX - 10);
    t.priority = 1; // lowest weight → fastest vruntime growth
    rq.enqueue(t);

    // This delta would overflow a plain u64; saturating_add should prevent it.
    let updated = rq.update_vruntime(ThreadId(1), u64::MAX);
    assert!(updated);
    let result = rq.dequeue_min().unwrap();
    assert_eq!(result.vruntime, u64::MAX, "vruntime should saturate at u64::MAX");
}
