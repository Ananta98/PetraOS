// ── Module shim: expose crate::sched as the test binary's crate path ─────────
//
// The source files use `crate::sched::task`, `crate::sched::cfs`, etc.
// We recreate that exact module hierarchy here so the imports resolve
// identically to how they would inside the kernel binary.

#[path = "."]
pub mod sched {
    #[path = "../../src/sched/task.rs"]
    pub mod task;

    #[path = "../../src/sched/cfs.rs"]
    pub mod cfs;
}

use sched::task::{Task, TaskId};
use sched::cfs::CfsRunQueue;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn task(id: u64, vruntime: u64) -> Task {
    let mut t = Task::new_normal(TaskId(id));
    t.vruntime = vruntime;
    t
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Enqueue three tasks with distinct vruntimes; verify dequeue is min-first.
#[test]
fn test_cfs_enqueue_dequeue_order() {
    let mut rq = CfsRunQueue::new();

    rq.enqueue(task(1, 300));
    rq.enqueue(task(2, 100));
    rq.enqueue(task(3, 200));

    let a = rq.dequeue_min().expect("should have task");
    let b = rq.dequeue_min().expect("should have task");
    let c = rq.dequeue_min().expect("should have task");

    assert_eq!(a.id, TaskId(2), "min vruntime (100) should dequeue first");
    assert_eq!(b.id, TaskId(3), "next min vruntime (200) should be second");
    assert_eq!(c.id, TaskId(1), "largest vruntime (300) should be last");

    assert!(rq.is_empty());
}

/// pick_next returns the minimum without removing the task.
#[test]
fn test_cfs_pick_next_does_not_remove() {
    let mut rq = CfsRunQueue::new();

    rq.enqueue(task(10, 50));
    rq.enqueue(task(20, 25));

    let peeked_id = rq.pick_next().expect("should peek").id;
    assert_eq!(peeked_id, TaskId(20));
    assert_eq!(rq.len(), 2, "pick_next must not remove the task");
}

/// After updating vruntime the task moves to its new sorted position.
#[test]
fn test_cfs_vruntime_update_changes_order() {
    let mut rq = CfsRunQueue::new();

    rq.enqueue(task(1, 0));
    rq.enqueue(task(2, 500));

    // Task 1 has vruntime = 0, task 2 = 500.  Advance task 1 by a big delta
    // so that it overtakes task 2.
    // weight = NICE_0_WEIGHT (1024), delta = 600 ns
    // vruntime_delta = 600 * 1024 / 1024 = 600  → new vruntime = 600
    let delta_ns = 600u64;
    let updated = rq.update_vruntime(TaskId(1), delta_ns);
    assert!(updated, "task must be found for vruntime update");

    // Now task 2 (500) < task 1 (600), so task 2 should dequeue first.
    let first = rq.dequeue_min().expect("task");
    assert_eq!(first.id, TaskId(2));
    let second = rq.dequeue_min().expect("task");
    assert_eq!(second.id, TaskId(1));
}

/// New tasks are lifted to min_vruntime so they don't jump ahead of existing tasks.
#[test]
fn test_cfs_new_task_lifted_to_min_vruntime() {
    let mut rq = CfsRunQueue::new();

    // Enqueue a task at vruntime 1000, then dequeue it → min_vruntime advances to 1000.
    let mut t = task(1, 0);
    t.vruntime = 1000;
    rq.enqueue(t);
    let _ = rq.dequeue_min(); // min_vruntime becomes 1000

    // A brand-new task with vruntime = 0 should be lifted to 1000.
    rq.enqueue(task(2, 0));
    let peeked = rq.pick_next().expect("should have task");
    assert_eq!(
        peeked.vruntime, 1000,
        "new task vruntime must be lifted to min_vruntime"
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

/// Removing a task by id works even when it is not the minimum.
#[test]
fn test_cfs_remove_by_id() {
    let mut rq = CfsRunQueue::new();

    rq.enqueue(task(1, 100));
    rq.enqueue(task(2, 200));
    rq.enqueue(task(3, 300));

    let removed = rq.remove(TaskId(2)).expect("task 2 should be removable");
    assert_eq!(removed.id, TaskId(2));
    assert_eq!(rq.len(), 2);

    // Remaining tasks should still be in order.
    let a = rq.dequeue_min().unwrap();
    let b = rq.dequeue_min().unwrap();
    assert_eq!(a.id, TaskId(1));
    assert_eq!(b.id, TaskId(3));
}

/// Removing a task that does not exist returns None and leaves the queue intact.
#[test]
fn test_cfs_remove_nonexistent_returns_none() {
    let mut rq = CfsRunQueue::new();
    rq.enqueue(task(1, 0));

    let result = rq.remove(TaskId(999));
    assert!(result.is_none());
    assert_eq!(rq.len(), 1);
}

/// update_vruntime on a non-existent task returns false.
#[test]
fn test_cfs_update_vruntime_nonexistent() {
    let mut rq = CfsRunQueue::new();
    assert!(!rq.update_vruntime(TaskId(42), 1000));
}

/// Saturating arithmetic prevents vruntime from overflowing.
#[test]
fn test_cfs_vruntime_saturates_on_overflow() {
    let mut rq = CfsRunQueue::new();

    let mut t = task(1, u64::MAX - 10);
    t.priority = 1; // lowest weight → fastest vruntime growth
    rq.enqueue(t);

    // This delta would overflow a plain u64; saturating_add should prevent it.
    let updated = rq.update_vruntime(TaskId(1), u64::MAX);
    assert!(updated);
    let result = rq.dequeue_min().unwrap();
    assert_eq!(result.vruntime, u64::MAX, "vruntime should saturate at u64::MAX");
}
