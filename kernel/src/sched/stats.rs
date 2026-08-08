/// Statistics and metrics tracked by the kernel scheduler.
#[derive(Debug, Clone, Copy, Default)]
pub struct SchedulerStats {
    /// Total number of context switches performed.
    pub total_context_switches: u64,
    /// Total number of voluntary yields requested by threads.
    pub total_yields: u64,
    /// Total number of thread block operations.
    pub total_blocks: u64,
    /// Total number of scheduler timer ticks processed.
    pub total_ticks: u64,
    /// Total number of threads added to the run queue.
    pub total_threads_added: u64,
    /// Total number of threads removed from the run queue.
    pub total_threads_removed: u64,
}

impl SchedulerStats {
    pub const fn new() -> Self {
        Self {
            total_context_switches: 0,
            total_yields: 0,
            total_blocks: 0,
            total_ticks: 0,
            total_threads_added: 0,
            total_threads_removed: 0,
        }
    }

    pub fn inc_context_switches(&mut self) {
        self.total_context_switches += 1;
    }

    pub fn inc_yields(&mut self) {
        self.total_yields += 1;
    }

    pub fn inc_blocks(&mut self) {
        self.total_blocks += 1;
    }

    pub fn inc_ticks(&mut self) {
        self.total_ticks += 1;
    }

    pub fn inc_threads_added(&mut self) {
        self.total_threads_added += 1;
    }

    pub fn inc_threads_removed(&mut self) {
        self.total_threads_removed += 1;
    }
}
