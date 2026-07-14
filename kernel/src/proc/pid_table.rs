use crate::proc::process::Process;
use alloc::collections::BTreeMap;
use core::sync::atomic::{AtomicU32, Ordering};
use ostd::sync::SpinLock;

static NEXT_PID: AtomicU32 = AtomicU32::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pid(u32);

impl Pid {
    pub fn new() -> Self {
        Self(NEXT_PID.fetch_add(1, Ordering::Relaxed))
    }

    pub fn as_u32(&self) -> u32 {
        self.0
    }

    /// Construct a `Pid` from a known raw value.
    ///
    /// Use sparingly — only for well-known PIDs (e.g. `1` for init).
    /// Does **not** allocate through `NEXT_PID`.
    pub fn from_raw(raw: u32) -> Self {
        Self(raw)
    }
}

pub struct ProcessTable {
    table: SpinLock<BTreeMap<Pid, Process>>,
}

impl ProcessTable {
    pub const fn new() -> Self {
        Self {
            table: SpinLock::new(BTreeMap::new()),
        }
    }

    pub fn register_process(&self, proc: Process) {
        let mut table = self.table.lock();
        table.insert(proc.pid, proc);
    }

    pub fn unregister_process(&self, pid: Pid) {
        let mut table = self.table.lock();
        table.remove(&pid);
    }

    pub fn get_process(&self, pid: Pid) -> Option<Process> {
        let table = self.table.lock();
        table.get(&pid).cloned()
    }

    pub fn update_process<F>(&self, pid: Pid, f: F)
    where
        F: FnOnce(&mut Process),
    {
        let mut table = self.table.lock();
        if let Some(proc) = table.get_mut(&pid) {
            f(proc);
        }
    }
}

impl Drop for ProcessTable {
    fn drop(&mut self) {
        let mut table = self.table.lock();
        table.clear();
    }
}

pub static PROCESS_TABLE: ProcessTable = ProcessTable::new();
