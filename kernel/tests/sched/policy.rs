// ── Module shim: expose crate::sched as the test binary's crate path ─────────

#[path = "."]
pub mod sched {
    #[path = "../../src/sched/task.rs"]
    pub mod task;

    #[path = "../../src/sched/cfs.rs"]
    pub mod cfs;

    #[path = "../../src/sched/rt.rs"]
    pub mod rt;

    #[path = "../../src/sched/policy.rs"]
    pub mod policy;
}

use sched::task::{Task, TaskId};
use sched::policy::PerCpuScheduler;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn normal(id: u64) -> Task {
    Task::new_normal(TaskId(id))
}

fn fifo(id: u64, prio: u32) -> Task {
    Task::new_fifo(TaskId(id), prio)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// RT task preempts a CFS task when both are runnable.
#[test]
fn test_policy_rt_preempts_cfs() {
    let mut cpu = PerCpuScheduler::new(0);

    cpu.add_task(normal(1));
    cpu.add_task(fifo(2, 50));

    let chosen = cpu.schedule().expect("should schedule something");
    assert_eq!(chosen, TaskId(2), "RT task (FIFO prio 50) must preempt the CFS task");
}

/// With no RT tasks present, the CFS task is scheduled.
#[test]
fn test_policy_fallback_to_cfs() {
    let mut cpu = PerCpuScheduler::new(0);

    cpu.add_task(normal(1));

    let chosen = cpu.schedule().expect("should schedule the CFS task");
    assert_eq!(chosen, TaskId(1));
}

/// Scheduling with no tasks returns None (idle CPU).
#[test]
fn test_policy_idle_returns_none() {
    let mut cpu = PerCpuScheduler::new(0);
    assert!(cpu.schedule().is_none());
}

/// Add then remove a task; CPU should report idle.
#[test]
fn test_policy_add_remove_task() {
    let mut cpu = PerCpuScheduler::new(0);

    cpu.add_task(normal(5));
    assert_eq!(cpu.runnable_count(), 1);

    cpu.remove_task(TaskId(5));
    assert_eq!(cpu.runnable_count(), 0);
    assert!(cpu.schedule().is_none());
}

/// Removing the running task clears the running slot.
#[test]
fn test_policy_remove_running_task_clears_slot() {
    let mut cpu = PerCpuScheduler::new(0);

    cpu.add_task(normal(7));
    let chosen = cpu.schedule().unwrap();
    assert_eq!(cpu.running_task(), Some(TaskId(7)));

    cpu.remove_task(chosen);
    assert!(cpu.running_task().is_none());
}

/// has_rt_tasks and has_cfs_tasks reflect queue state correctly.
#[test]
fn test_policy_has_rt_and_cfs_flags() {
    let mut cpu = PerCpuScheduler::new(0);

    assert!(!cpu.has_rt_tasks());
    assert!(!cpu.has_cfs_tasks());

    cpu.add_task(normal(1));
    assert!(cpu.has_cfs_tasks());
    assert!(!cpu.has_rt_tasks());

    cpu.add_task(fifo(2, 10));
    assert!(cpu.has_rt_tasks());
}

/// A higher-priority RT task preempts a lower-priority one.
#[test]
fn test_policy_rt_higher_priority_first() {
    let mut cpu = PerCpuScheduler::new(0);

    cpu.add_task(fifo(1, 10));
    cpu.add_task(fifo(2, 80));
    cpu.add_task(fifo(3, 40));

    let first = cpu.schedule().unwrap();
    assert_eq!(first, TaskId(2), "highest RT priority (80) must be chosen");
}

/// CFS schedules min-vruntime first across multiple tasks.
#[test]
fn test_policy_cfs_min_vruntime_order() {
    let mut cpu = PerCpuScheduler::new(0);

    let mut t1 = normal(1);
    t1.vruntime = 500;
    let mut t2 = normal(2);
    t2.vruntime = 100;
    let mut t3 = normal(3);
    t3.vruntime = 300;

    cpu.add_task(t1);
    cpu.add_task(t2);
    cpu.add_task(t3);

    let first = cpu.schedule().unwrap();
    assert_eq!(first, TaskId(2), "CFS must pick task with smallest vruntime");
}

/// cpu_id is stored and accessible.
#[test]
fn test_policy_cpu_id_stored() {
    let cpu = PerCpuScheduler::new(3);
    assert_eq!(cpu.cpu_id, 3);
}
