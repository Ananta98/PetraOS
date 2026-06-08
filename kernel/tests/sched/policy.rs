// ── Module shim: expose crate::sched as the test binary's crate path ─────────

#[path = "."]
pub mod sched {
    #[path = "../../src/sched/sched_thread.rs"]
    pub mod sched_thread;

    #[path = "../../src/sched/cfs.rs"]
    pub mod cfs;

    #[path = "../../src/sched/rt.rs"]
    pub mod rt;

    #[path = "../../src/sched/policy.rs"]
    pub mod policy;
}

use sched::sched_thread::{SchedThread, ThreadId};
use sched::policy::PerCpuScheduler;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn normal(id: u64) -> SchedThread {
    SchedThread::new_normal(ThreadId(id))
}

fn fifo(id: u64, prio: u32) -> SchedThread {
    SchedThread::new_fifo(ThreadId(id), prio)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// RT thread preempts a CFS thread when both are runnable.
#[test]
fn test_policy_rt_preempts_cfs() {
    let mut cpu = PerCpuScheduler::new(0);

    cpu.add_thread(normal(1));
    cpu.add_thread(fifo(2, 50));

    let chosen = cpu.schedule().expect("should schedule something");
    assert_eq!(chosen, ThreadId(2), "RT thread (FIFO prio 50) must preempt the CFS thread");
}

/// With no RT threads present, the CFS thread is scheduled.
#[test]
fn test_policy_fallback_to_cfs() {
    let mut cpu = PerCpuScheduler::new(0);

    cpu.add_thread(normal(1));

    let chosen = cpu.schedule().expect("should schedule the CFS thread");
    assert_eq!(chosen, ThreadId(1));
}

/// Scheduling with no threads returns None (idle CPU).
#[test]
fn test_policy_idle_returns_none() {
    let mut cpu = PerCpuScheduler::new(0);
    assert!(cpu.schedule().is_none());
}

/// Add then remove a thread; CPU should report idle.
#[test]
fn test_policy_add_remove_task() {
    let mut cpu = PerCpuScheduler::new(0);

    cpu.add_thread(normal(5));
    assert_eq!(cpu.runnable_count(), 1);

    cpu.remove_thread(ThreadId(5));
    assert_eq!(cpu.runnable_count(), 0);
    assert!(cpu.schedule().is_none());
}

/// Removing the running thread clears the running slot.
#[test]
fn test_policy_remove_running_task_clears_slot() {
    let mut cpu = PerCpuScheduler::new(0);

    cpu.add_thread(normal(7));
    let chosen = cpu.schedule().unwrap();
    assert_eq!(cpu.running_thread(), Some(ThreadId(7)));

    cpu.remove_thread(chosen);
    assert!(cpu.running_thread().is_none());
}

/// has_rt_tasks and has_cfs_tasks reflect queue state correctly.
#[test]
fn test_policy_has_rt_and_cfs_flags() {
    let mut cpu = PerCpuScheduler::new(0);

    assert!(!cpu.has_rt_tasks());
    assert!(!cpu.has_cfs_tasks());

    cpu.add_thread(normal(1));
    assert!(cpu.has_cfs_tasks());
    assert!(!cpu.has_rt_tasks());

    cpu.add_thread(fifo(2, 10));
    assert!(cpu.has_rt_tasks());
}

/// A higher-priority RT thread preempts a lower-priority one.
#[test]
fn test_policy_rt_higher_priority_first() {
    let mut cpu = PerCpuScheduler::new(0);

    cpu.add_thread(fifo(1, 10));
    cpu.add_thread(fifo(2, 80));
    cpu.add_thread(fifo(3, 40));

    let first = cpu.schedule().unwrap();
    assert_eq!(first, ThreadId(2), "highest RT priority (80) must be chosen");
}

/// CFS schedules min-vruntime first across multiple threads.
#[test]
fn test_policy_cfs_min_vruntime_order() {
    let mut cpu = PerCpuScheduler::new(0);

    let mut t1 = normal(1);
    t1.vruntime = 500;
    let mut t2 = normal(2);
    t2.vruntime = 100;
    let mut t3 = normal(3);
    t3.vruntime = 300;

    cpu.add_thread(t1);
    cpu.add_thread(t2);
    cpu.add_thread(t3);

    let first = cpu.schedule().unwrap();
    assert_eq!(first, ThreadId(2), "CFS must pick thread with smallest vruntime");
}

/// cpu_id is stored and accessible.
#[test]
fn test_policy_cpu_id_stored() {
    let cpu = PerCpuScheduler::new(3);
    assert_eq!(cpu.cpu_id, 3);
}
