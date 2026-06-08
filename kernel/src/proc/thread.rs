use crate::arch::{CpuArch, ArchImpl};
use crate::sched::sched_thread::ThreadId;
use crate::proc::process::ProcessId;
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
    /// Unique thread identifier.
    pub id: ThreadId,
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
    pub fn new(id: ThreadId, process_id: ProcessId, entry: extern "C" fn(*mut u8), arg: *mut u8) -> Self {
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
    pub fn new_boot(id: ThreadId, process_id: ProcessId) -> Self {
        Self {
            id,
            process_id,
            rsp: 0,
            stack: None,
            state: ThreadState::Running,
        }
    }
}
