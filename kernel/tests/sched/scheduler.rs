// ── Module shim: expose crate::sched + crate::sync ───────────────────────────

#[path = "."]
pub mod sync {
    #[path = "."]
    pub mod spinlock {
        #[path = "../../src/sync/spinlock.rs"]
        pub mod impl_spinlock;
        pub use impl_spinlock::Spinlock;
    }
}

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

    #[path = "../../src/sched/scheduler.rs"]
    pub mod scheduler;
}

use sched::task::{Task, TaskId};
use sched::scheduler::{GlobalScheduler, tick_and_schedule, GLOBAL_SCHEDULER};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn normal(id: u64) -> Task {
    Task::new_normal(TaskId(id))
}

fn fifo(id: u64, prio: u32) -> Task {
    Task::new_fifo(TaskId(id), prio)
}

/// Build a two-CPU global scheduler ready for testing.
fn two_cpu_scheduler() -> GlobalScheduler {
    let mut gs = GlobalScheduler::new();
    gs.register_cpu(0);
    gs.register_cpu(1);
    gs
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// register_cpu adds CPUs and prevents double-registration.
#[test]
fn test_global_register_cpu() {
    let mut gs = GlobalScheduler::new();

    assert!(gs.register_cpu(0));
    assert!(gs.register_cpu(1));
    assert!(!gs.register_cpu(1), "double registration must return false");
    assert_eq!(gs.cpu_count(), 2);
}

/// spawn_task with no preferred CPU uses the least-loaded CPU.
#[test]
fn test_global_spawn_load_balance() {
    let mut gs = two_cpu_scheduler();

    // CPU 0 should get the first task; CPU 1 should get the second.
    let cpu_a = gs.spawn_task(normal(1), None).expect("must place task");
    let cpu_b = gs.spawn_task(normal(2), None).expect("must place task");

    // Both CPUs are empty initially, so the first two tasks go to different CPUs.
    assert_ne!(cpu_a, cpu_b, "load balancer must spread tasks across idle CPUs");
    assert_eq!(gs.total_runnable(), 2);
}

/// spawn_task honours a preferred CPU when it has equal or minimal load.
#[test]
fn test_global_spawn_preferred_cpu() {
    let mut gs = two_cpu_scheduler();

    let assigned = gs
        .spawn_task(normal(1), Some(1))
        .expect("must place task");
    assert_eq!(assigned, 1, "task should land on the preferred CPU");
}

/// schedule on each CPU works independently.
#[test]
fn test_global_schedule_per_cpu_independent() {
    let mut gs = two_cpu_scheduler();

    gs.spawn_task(normal(1), Some(0)).unwrap();
    gs.spawn_task(normal(2), Some(1)).unwrap();

    let chosen_0 = gs.schedule(0).expect("CPU 0 should schedule task 1");
    let chosen_1 = gs.schedule(1).expect("CPU 1 should schedule task 2");

    assert_eq!(chosen_0, TaskId(1));
    assert_eq!(chosen_1, TaskId(2));
}

/// RT task on one CPU does not affect scheduling on another CPU.
#[test]
fn test_global_rt_isolation_between_cpus() {
    let mut gs = two_cpu_scheduler();

    gs.spawn_task(normal(1), Some(0)).unwrap();
    gs.spawn_task(fifo(2, 99), Some(1)).unwrap(); // RT on CPU 1 only

    // CPU 0 has only a CFS task → should pick it.
    let chosen_0 = gs.schedule(0).expect("CPU 0 should have a CFS task");
    assert_eq!(chosen_0, TaskId(1), "CPU 0 CFS task must not be preempted by CPU 1 RT");

    // CPU 1 has only an RT task → should pick it.
    let chosen_1 = gs.schedule(1).expect("CPU 1 should have an RT task");
    assert_eq!(chosen_1, TaskId(2));
}

/// remove_task works across CPUs.
#[test]
fn test_global_remove_task() {
    let mut gs = two_cpu_scheduler();

    gs.spawn_task(normal(10), Some(0)).unwrap();
    gs.spawn_task(normal(20), Some(1)).unwrap();

    let (removed, cpu) = gs.remove_task(TaskId(10)).expect("task 10 must exist");
    assert_eq!(removed.id, TaskId(10));
    assert_eq!(cpu, 0);
    assert_eq!(gs.total_runnable(), 1);
}

/// Scheduling on an unregistered CPU returns None.
#[test]
fn test_global_schedule_unregistered_cpu_returns_none() {
    let mut gs = GlobalScheduler::new();
    gs.register_cpu(0);

    // CPU 7 was never registered.
    assert!(gs.schedule(7).is_none());
}

/// spawn_task with no registered CPUs returns None.
#[test]
fn test_global_spawn_no_cpus_returns_none() {
    let mut gs = GlobalScheduler::new();
    let result = gs.spawn_task(normal(1), None);
    assert!(result.is_none());
}

/// total_runnable tracks tasks correctly across multiple CPUs.
#[test]
fn test_global_total_runnable_count() {
    let mut gs = two_cpu_scheduler();

    assert_eq!(gs.total_runnable(), 0);

    gs.spawn_task(normal(1), Some(0)).unwrap();
    gs.spawn_task(normal(2), Some(0)).unwrap();
    gs.spawn_task(fifo(3, 10), Some(1)).unwrap();

    assert_eq!(gs.total_runnable(), 3);

    gs.schedule(0); // dequeues one task from CPU 0
    assert_eq!(gs.total_runnable(), 2);
}

/// task_tick does not panic on an idle CPU.
#[test]
fn test_global_task_tick_idle_cpu_no_panic() {
    let mut gs = two_cpu_scheduler();
    // No task running — tick should be a safe no-op.
    gs.task_tick(0, 1_000_000);
}

/// tick_and_schedule on an idle CPU returns None.
#[test]
fn test_tick_and_schedule_idle_returns_none() {
    // Register CPU 0 in the global singleton.
    GLOBAL_SCHEDULER.lock().register_cpu(0);
    // No tasks — tick returns None.
    let result = tick_and_schedule(0);
    assert!(result.is_none(), "idle CPU must return None from tick_and_schedule");
}

/// tick_and_schedule advances vruntime and returns a task when one is queued.
#[test]
fn test_tick_and_schedule_with_task() {
    // CPU 1 must be registered; re-registration is silently ignored.
    GLOBAL_SCHEDULER.lock().register_cpu(1);
    GLOBAL_SCHEDULER.lock().spawn_task(normal(100), Some(1));

    let chosen = tick_and_schedule(1);
    assert!(chosen.is_some(), "tick_and_schedule must return Some when a task is queued");
    assert_eq!(chosen.unwrap(), TaskId(100));
}
