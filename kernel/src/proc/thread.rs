use crate::arch::{CpuArch, ArchImpl};
use crate::sched::task::{Task, TaskId};
use crate::sched::MAX_CPUS;
use crate::sync::spinlock::Spinlock;
use crate::proc::process::{ProcessId, PROCESS_MANAGER};
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use alloc::vec;

/// The execution state of a thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadState {
    Ready,
    Running,
    Blocked,
    Terminated,
}

/// A kernel thread descriptor.
pub struct Thread {
    /// Unique task/thread identifier.
    pub id: TaskId,
    /// Owning process identifier.
    pub process_id: ProcessId,
    /// Saved stack pointer.
    pub rsp: u64,
    /// The backing stack memory.
    ///
    /// Set to `None` for the initial boot/main threads which use pre-allocated stacks.
    pub stack: Option<Vec<u8>>,
    /// Current execution state of the thread.
    pub state: ThreadState,
}

impl Thread {
    /// Create a new kernel thread with its own allocated stack.
    pub fn new(id: TaskId, process_id: ProcessId, entry: extern "C" fn(*mut u8), arg: *mut u8) -> Self {
        const STACK_SIZE: usize = 65536; // 64 KiB stack
        let mut stack = vec![0u8; STACK_SIZE];

        // Initialize stack frame through the architecture abstraction layer
        let rsp = ArchImpl::init_stack(&mut stack, entry, arg);

        Self {
            id,
            process_id,
            rsp,
            stack: Some(stack),
            state: ThreadState::Ready,
        }
    }

    /// Create a placeholder Thread for an already running context (like a boot/main thread).
    pub fn new_boot(id: TaskId, process_id: ProcessId) -> Self {
        Self {
            id,
            process_id,
            rsp: 0,
            stack: None,
            state: ThreadState::Running,
        }
    }
}

// ── Thread Manager ───────────────────────────────────────────────────────────

/// OOP Manager for tracking threads, state transitions, and context routing.
pub struct ThreadManager {
    pub threads: BTreeMap<TaskId, Thread>,
    current_threads: [Option<TaskId>; MAX_CPUS],
}

impl ThreadManager {
    /// Create a new thread manager.
    pub const fn new() -> Self {
        Self {
            threads: BTreeMap::new(),
            current_threads: [None; MAX_CPUS],
        }
    }

    /// Initialize the thread registry for a CPU core.
    pub fn init_threads(&mut self, cpu_id: u32) {
        let main_tid = TaskId(cpu_id as u64);
        let idle_tid = TaskId((cpu_id + 100) as u64);
        let init_pid = ProcessId(1);

        // Ensure ProcessId(1) (the kernel/init process) exists.
        {
            let mut pm = PROCESS_MANAGER.lock();
            if pm.get_process(init_pid).is_none() {
                let pid = pm.create_process(None);
                assert_eq!(pid, init_pid);
            }
            let _ = pm.add_thread_to_process(init_pid, main_tid);
            let _ = pm.add_thread_to_process(init_pid, idle_tid);
        }

        // 1. Create boot/main thread placeholder
        let boot_thread = Thread::new_boot(main_tid, init_pid);
        self.threads.insert(main_tid, boot_thread);
        self.current_threads[cpu_id as usize] = Some(main_tid);

        // 2. Create idle thread for this CPU
        let idle_thread = Thread::new(idle_tid, init_pid, idle_thread_entry, core::ptr::null_mut());
        self.threads.insert(idle_tid, idle_thread);

        // 3. Register the main thread as the running task in the scheduler
        let mut sched = crate::sched::scheduler::GLOBAL_SCHEDULER.lock();
        let main_task = Task::new_normal(main_tid);
        sched.set_running_task(cpu_id, main_task);
    }

    /// Spawn a new thread inside the registry and associate it with a process.
    pub fn spawn_thread(&mut self, id: TaskId, pid: ProcessId, entry: extern "C" fn(*mut u8), arg: *mut u8) {
        let thread = Thread::new(id, pid, entry, arg);
        self.threads.insert(id, thread);

        let mut pm = PROCESS_MANAGER.lock();
        let _ = pm.add_thread_to_process(pid, id);
    }

    /// Prepare registry and process states for current thread termination.
    pub fn exit_current_thread_prepare(&mut self, cpu_id: u32, exit_code: i32) -> TaskId {
        let tid = self.current_threads[cpu_id as usize].expect("No running thread on this CPU");

        // Retrieve process ID of exiting thread and remove thread from process
        if let Some(thread) = self.threads.get(&tid) {
            let pid = thread.process_id;
            let mut pm = PROCESS_MANAGER.lock();
            let _ = pm.remove_thread_from_process(pid, tid);
            if pid != ProcessId(1) {
                if let Some(proc) = pm.get_process(pid) {
                    if proc.threads().is_empty() {
                        // Last thread exited, transition process to zombie.
                        let _ = pm.exit_process(pid, exit_code);
                    }
                }
            }
        }

        // Mark current thread as Terminated in registry
        if let Some(thread) = self.threads.get_mut(&tid) {
            thread.state = ThreadState::Terminated;
        }

        tid
    }

    /// Prepare context switch details and update states, returning rsp pointers.
    pub fn switch_to_prepare(&mut self, cpu_id: u32, next_id: TaskId) -> Option<(*mut u64, u64)> {
        let current_id_opt = self.current_threads[cpu_id as usize];
        if current_id_opt == Some(next_id) {
            return None; // Already running target thread
        }

        let prev_ptr = if let Some(curr_id) = current_id_opt {
            if let Some(thread) = self.threads.get_mut(&curr_id) {
                if thread.state == ThreadState::Running {
                    thread.state = ThreadState::Ready;
                }
                &mut thread.rsp as *mut u64
            } else {
                core::ptr::null_mut()
            }
        } else {
            core::ptr::null_mut()
        };

        let next_thread = self.threads.get_mut(&next_id).expect("Next thread not found in registry");
        next_thread.state = ThreadState::Running;
        let next_val = next_thread.rsp;

        // Update current thread tracking
        self.current_threads[cpu_id as usize] = Some(next_id);

        Some((prev_ptr, next_val))
    }

    /// Returns the current thread ID executing on the specified CPU.
    pub fn current_thread_id(&self, cpu_id: u32) -> Option<TaskId> {
        self.current_threads[cpu_id as usize]
    }
}

/// The global thread manager singleton.
pub static THREAD_MANAGER: Spinlock<ThreadManager> = Spinlock::new(ThreadManager::new());

// ── Thread Lifecycle APIs (Public wrappers) ───────────────────────────────────

/// Initialize the thread subsystem for the calling CPU.
pub fn init_threads(cpu_id: u32) {
    THREAD_MANAGER.lock().init_threads(cpu_id);
}

/// The entry point for the per-CPU idle thread.
extern "C" fn idle_thread_entry(_arg: *mut u8) {
    loop {
        ArchImpl::halt();
    }
}

/// Spawn a new thread in the global registry.
pub fn spawn_thread(id: TaskId, pid: ProcessId, entry: extern "C" fn(*mut u8), arg: *mut u8) {
    THREAD_MANAGER.lock().spawn_thread(id, pid, entry, arg);
}

/// Terminate the currently executing thread on the calling CPU.
pub fn exit_current_thread(exit_code: i32) -> ! {
    let cpu_id = ArchImpl::cpu_id();

    // Prepare termination state
    {
        THREAD_MANAGER.lock().exit_current_thread_prepare(cpu_id, exit_code);
    }

    // Deassociate the running task in the scheduler to prevent it from being re-enqueued
    {
        let mut sched = crate::sched::scheduler::GLOBAL_SCHEDULER.lock();
        if let Some(cpu) = sched.cpu_mut(cpu_id) {
            let _prev = cpu.running.take();
        }
    }

    // Trigger context switch to the next task
    yield_now();

    // Safety fallback: if there are no tasks and yield returns, park the CPU
    loop {
        ArchImpl::halt();
    }
}

/// Yield the CPU voluntarily to another runnable task.
pub fn yield_now() {
    let cpu_id = ArchImpl::cpu_id();

    // Disable interrupts during thread state transitions and context switching
    let ints = ArchImpl::disable_interrupts();

    let next_id = {
        let mut sched = crate::sched::scheduler::GLOBAL_SCHEDULER.lock();
        // Charge the yielding CFS task a virtual runtime penalty to allow other tasks to run
        if let Some(cpu) = sched.cpu_mut(cpu_id) {
            if let Some(ref mut task) = cpu.running {
                if !task.policy.is_realtime() {
                    task.vruntime = task.vruntime.saturating_add(10_000_000); // 10 ms equivalent
                }
            }
        }
        sched.schedule(cpu_id)
    };

    let target_id = match next_id {
        Some(tid) => tid,
        None => TaskId((cpu_id + 100) as u64), // Idle thread
    };

    switch_to(cpu_id, target_id);

    if ints {
        ArchImpl::enable_interrupts();
    }
}

/// Perform context switch to target thread `next_id` on CPU `cpu_id`.
pub fn switch_to(cpu_id: u32, next_id: TaskId) {
    let prep = {
        THREAD_MANAGER.lock().switch_to_prepare(cpu_id, next_id)
    };

    if let Some((prev_rsp_ptr, next_rsp)) = prep {
        unsafe {
            if !prev_rsp_ptr.is_null() {
                ArchImpl::switch_context(prev_rsp_ptr, next_rsp);
            } else {
                ArchImpl::switch_context_to(next_rsp);
            }
        }
    }
}

/// Returns the current thread ID executing on the specified CPU.
pub fn current_thread_id(cpu_id: u32) -> Option<TaskId> {
    THREAD_MANAGER.lock().current_thread_id(cpu_id)
}
