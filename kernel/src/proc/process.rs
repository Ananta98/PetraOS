use crate::fs::fd_table::FdTable;
use crate::ipc::ProcessSignals;
use crate::proc::elf::{LoadedElf, load_elf_image};
use crate::proc::pid_table::{PROCESS_TABLE, Pid};
use crate::proc::thread::KernelThread;
use crate::proc::tid_table::Tid;
use crate::vm::vma::VmaManager;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use ostd::Error;
use ostd::sync::SpinLock;

// ---------------------------------------------------------------------------
// ProcessState
// ---------------------------------------------------------------------------

/// Unix-compatible process lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    /// Runnable / waiting in scheduler queue.
    Ready,
    /// Currently executing on a CPU.
    Running,
    /// Blocked on I/O or a condition.
    Sleeping,
    /// Exited but not yet reaped by parent.
    Zombie,
}

// ---------------------------------------------------------------------------
// Process
// ---------------------------------------------------------------------------

/// A kernel process descriptor, analogous to the Linux `task_struct` /
/// BSD `proc` structure.
///
/// # argv / envp
/// Command-line arguments and environment variables are **not** stored
/// inside this struct.  In a real Unix system they live in the process's
/// user-space stack and are set up by the program loader (`execve`).
/// PetraOS will follow the same convention: when an ELF loader is added,
/// it will map the initial stack with `argc`, `argv[]`, and `envp[]`
/// vectors at the bottom of user-space memory.  The kernel itself only
/// needs the *executable path* (stored as the process `name`).
///
/// # Memory ownership
/// - The parent holds strong refs to children through `Process` handles.
/// - Children keep only the parent PID, avoiding unnecessary weak refs.
///
/// # Thread model
/// A process is the **owner** of one or more kernel threads.  The first
/// thread (the main thread) is automatically created when a process is
/// spawned.  Additional threads can be added with [`Process::spawn_thread`].
/// All threads share the process's VM and FD table.
#[derive(Clone)]
pub struct Process {
    /// Unique process identifier.
    pub pid: Pid,

    /// Parent process ID (by value — no strong ref).
    pub ppid: Option<Pid>,

    /// Real user ID.
    pub uid: u32,

    /// Effective user ID.
    pub euid: u32,

    /// Saved set-user-ID.
    pub suid: u32,

    /// Filesystem user ID.
    pub fsuid: u32,

    /// Real group ID.
    pub gid: u32,

    /// Effective group ID.
    pub egid: u32,

    /// Saved set-group-ID.
    pub sgid: u32,

    /// Filesystem group ID.
    pub fsgid: u32,

    /// Process group ID.
    pub pgid: Pid,
    
    /// Session ID.
    pub sid: Pid,

    /// Child processes waiting to be waited on.
    pub children: Arc<SpinLock<Vec<Pid>>>,

    /// Current lifecycle state.
    pub state: ProcessState,

    /// Exit status set by `exit()`, harvested by the parent via
    /// `wait_child()`.
    pub exit_code: i32,

    /// Virtual memory address space.
    pub vm: Arc<VmaManager>,

    /// Short process name — the basename of the executable path
    /// (analogous to `comm` / `TASK_COMM_LEN` in Linux).
    pub name: String,

    /// File descriptor table mapping file descriptor numbers to open files.
    pub fd_table: Arc<SpinLock<FdTable>>,

    /// Kernel threads belonging to this process.
    ///
    /// Indexed by TID for O(log n) lookup.  The list is guarded by a
    /// `SpinLock` so that concurrent `spawn_thread` / `join_thread` calls
    /// are safe.
    pub threads: Arc<SpinLock<BTreeMap<Tid, Arc<KernelThread>>>>,

    /// Signal subsystem state: pending queue + installed action table.
    ///
    /// Wrapped in an `Arc` so that multiple handles to the same process (e.g.,
    /// threads, the dispatcher) can share the same signal state without
    /// cloning the entire `Process`.
    pub signals: Arc<ProcessSignals>,
}

// ---------------------------------------------------------------------------
// Constructors
// ---------------------------------------------------------------------------

impl Process {
    /// Create a **new** top-level process (no parent).
    ///
    /// A fresh `Pid` is allocated automatically and the process is
    /// immediately registered in the global `PROCESS_TABLE`.
    ///
    /// # Arguments
    /// * `vm`   – Virtual memory manager for this process.
    /// * `name` – Short executable name (basename of the path that will be
    ///            exec'd).  Analogous to Linux's `task_struct::comm`.
    pub fn new(vm: Arc<VmaManager>, name: &str) -> Process {
        let pid = Pid::new();
        let proc = Process {
            pid,
            ppid: None,
            uid: 0,
            euid: 0,
            suid: 0,
            fsuid: 0,
            gid: 0,
            egid: 0,
            sgid: 0,
            fsgid: 0,
            pgid: pid,
            sid: pid,
            children: Arc::new(SpinLock::new(Vec::new())),
            state: ProcessState::Ready,
            exit_code: 0,
            vm,
            name: String::from(name),
            fd_table: Arc::new(SpinLock::new(FdTable::new())),
            threads: Arc::new(SpinLock::new(BTreeMap::new())),
            signals: Arc::new(ProcessSignals::new()),
        };

        PROCESS_TABLE.register_process(proc.clone());
        proc
    }

    /// Create a **child** process with an explicit parent.
    ///
    /// The parent's PID is stored in `ppid`. The child inherits the
    /// parent's name (it will be overwritten by `execve` once a
    /// program-loader layer exists). The new process is registered in
    /// `PROCESS_TABLE` and added to the parent's child list.
    fn new_child(parent: &Process, vm: Arc<VmaManager>) -> Process {
        let child_pid = Pid::new();
        let child = Process {
            pid: child_pid,
            ppid: Some(parent.pid),
            uid: parent.uid,
            euid: parent.euid,
            suid: parent.suid,
            fsuid: parent.fsuid,
            gid: parent.gid,
            egid: parent.egid,
            sgid: parent.sgid,
            fsgid: parent.fsgid,
            pgid: parent.pgid,
            sid: parent.sid,
            children: Arc::new(SpinLock::new(Vec::new())),
            state: ProcessState::Ready,
            exit_code: 0,
            vm,
            // Inherit parent name; execve will overwrite this later.
            name: parent.name.clone(),
            fd_table: Arc::new(SpinLock::new(parent.fd_table.lock().clone())),
            // Child starts with its own empty thread list; threads are not
            // inherited across fork() — they must be re-created in the child.
            threads: Arc::new(SpinLock::new(BTreeMap::new())),
            // Each child gets a fresh, independent signal state.
            signals: Arc::new(ProcessSignals::new()),
        };

        PROCESS_TABLE.register_process(child.clone());
        parent.children.lock().push(child.pid);
        child
    }

    // ---------------------------------------------------------------------------
    // Unix-like lifecycle methods
    // ---------------------------------------------------------------------------

    /// **`fork()`** — create a copy-on-write clone of this process.
    ///
    /// The child gets:
    /// - A CoW-cloned virtual address space (`fork_vm_space`).
    /// - A new PID.
    /// - The parent's PID is stored in `ppid`.
    /// - The parent's `name` (overwritten on `execve` in a future layer).
    ///
    /// Returns `Ok(Process)` with the child, or an error if VM
    /// cloning fails.
    pub fn fork(&self) -> Result<Process, ostd::Error> {
        let child_vm = self.vm.fork_vm_space()?;
        Ok(Self::new_child(self, child_vm))
    }

    /// Replace the current executable image with a fresh ELF image.
    ///
    /// This is the process-level equivalent of `execve()`: it clears the
    /// existing address space, loads the new image into the VM, and updates
    /// the process name to the basename of the path being executed.
    pub fn exec(
        &mut self,
        path: &str,
        elf_image: &[u8],
        _argv: &[&str],
        _envp: &[&str],
    ) -> Result<LoadedElf, Error> {
        let region_specs: Vec<(usize, usize)> = {
            let regions = self.vm.regions.lock();
            regions
                .iter()
                .map(|(&start, region)| (start, region.size()))
                .collect()
        };

        for (start, size) in region_specs {
            self.vm.unmap_region(start, size)?;
        }

        let loaded = load_elf_image(&self.vm, elf_image)?;

        // Set the initial program break right after the loaded ELF image
        // (end of BSS), page-aligned.
        let page = ostd::mm::PAGE_SIZE;
        let heap_start = (loaded.load_end + page - 1) & !(page - 1);
        self.vm.set_brk_initial(heap_start);

        let executable_name = path.rfind('/').map_or(path, |i| &path[i + 1..]);
        self.name = String::from(executable_name);
        self.state = ProcessState::Ready;

        // Reset signal dispositions on exec: user handlers → SIG_DFL,
        // SIG_IGN is preserved (POSIX requirement).
        self.signals.reset_on_exec();

        let name_clone = self.name.clone();
        PROCESS_TABLE.update_process(self.pid, |p| {
            p.name = name_clone;
            p.state = ProcessState::Ready;
        });

        Ok(loaded)
    }

    /// **`exit(code)`** — terminate this process.
    ///
    /// Transitions the process state to `Zombie` and records the exit
    /// status.  The process remains in `PROCESS_TABLE` until the parent
    /// calls `wait_child()` to reap it.
    ///
    /// Orphaned children (if any) are reparented to the init process
    /// (PID 1), mirroring the Linux behaviour in `do_exit()`.
    pub fn exit(&mut self, code: i32) {
        self.state = ProcessState::Zombie;
        self.exit_code = code;

        PROCESS_TABLE.update_process(self.pid, |p| {
            p.state = ProcessState::Zombie;
            p.exit_code = code;
        });

        // Reparent any children to init (PID 1).
        let init_pid = Pid::from_raw(1);
        if let Some(init) = PROCESS_TABLE.get_process(init_pid) {
            let mut own_children = self.children.lock();
            let mut init_children = init.children.lock();
            for child_pid in own_children.drain(..) {
                if let Some(_) = PROCESS_TABLE.get_process(child_pid) {
                    PROCESS_TABLE.update_process(child_pid, |p| {
                        p.ppid = Some(init_pid);
                    });
                }
                init_children.push(child_pid);
            }
        }
    }

    /// **`wait_child(pid)`** — reap a zombie child.
    ///
    /// If `pid` is `None`, reaps *any* zombie child (analogous to
    /// `waitpid(-1, …)`).  If `pid` is `Some(p)`, waits specifically for
    /// child with PID `p`.
    ///
    /// Returns `Some((child_pid, exit_code))` when a zombie is found and
    /// reaped (removed from the child list and from `PROCESS_TABLE`), or
    /// `None` if no matching zombie exists yet.
    pub fn wait_child(&self, pid: Option<Pid>) -> Option<(Pid, i32)> {
        let mut children = self.children.lock();

        let pos = children.iter().position(|&child_pid| {
            let is_zombie = PROCESS_TABLE
                .get_process(child_pid)
                .map_or(false, |child| child.state == ProcessState::Zombie);
            let pid_matches = pid.map_or(true, |p| child_pid == p);
            is_zombie && pid_matches
        })?;

        let child_pid = children.remove(pos);
        let code = PROCESS_TABLE
            .get_process(child_pid)
            .map_or(0, |child| child.exit_code);
        PROCESS_TABLE.unregister_process(child_pid);
        Some((child_pid, code))
    }

    // -----------------------------------------------------------------------
    // Thread management
    // -----------------------------------------------------------------------

    /// Spawn a new kernel thread belonging to this process.
    ///
    /// `name` is a human-readable label used in diagnostics.  `func` is the
    /// thread body — it must be `Send + 'static` because the scheduler can
    /// migrate it across CPUs.
    ///
    /// The thread is registered in the global [`THREAD_TABLE`][crate::proc::tid_table::THREAD_TABLE]
    /// **and** in this process's `threads` map.  Call [`Process::join_thread`]
    /// to wait for completion and clean up the map entry.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the OSTD task allocation fails.
    pub fn spawn_thread<F>(&self, name: &str, func: F) -> Result<Arc<KernelThread>, Error>
    where
        F: FnOnce() + Send + 'static,
    {
        let thread = KernelThread::spawn(self.pid, name, func)?;
        self.threads.lock().insert(thread.tid(), thread.clone());
        Ok(thread)
    }

    /// Block until the given thread finishes, then remove it from the
    /// process's thread list.
    ///
    /// Returns the thread's exit code.  If the `tid` does not belong to this
    /// process, returns `None`.
    pub fn join_thread(&self, tid: Tid) -> Option<i32> {
        let thread = self.threads.lock().get(&tid).cloned()?;
        let code = thread.join();
        self.threads.lock().remove(&tid);
        Some(code)
    }

    // -----------------------------------------------------------------------
    // State transitions
    // -----------------------------------------------------------------------

    /// Mark process as Running (called by the scheduler when dispatching).
    pub fn set_running(&mut self) {
        if self.state == ProcessState::Ready {
            self.state = ProcessState::Running;
            PROCESS_TABLE.update_process(self.pid, |p| p.state = ProcessState::Running);
        }
    }

    /// Mark process as Sleeping (called when blocking on I/O / event).
    pub fn set_sleeping(&mut self) {
        if self.state == ProcessState::Running {
            self.state = ProcessState::Sleeping;
            PROCESS_TABLE.update_process(self.pid, |p| p.state = ProcessState::Sleeping);
        }
    }

    /// Wake a sleeping process back to Ready.
    pub fn wake_up(&mut self) {
        if self.state == ProcessState::Sleeping {
            self.state = ProcessState::Ready;
            PROCESS_TABLE.update_process(self.pid, |p| p.state = ProcessState::Ready);
        }
    }

    /// Get the current process executing in the current task context, or fall back to PID 1 (init).
    pub fn current() -> Process {
        if let Some(task) = ostd::task::Task::current() {
            if let Some(task_data) = task
                .data()
                .downcast_ref::<crate::proc::scheduler::TaskData>()
            {
                if let Some(proc) = PROCESS_TABLE.get_process(task_data.pid) {
                    return proc;
                }
            }
        }
        // Fallback: return PID 1 (init).
        PROCESS_TABLE
            .get_process(Pid::from_raw(1))
            .expect("init process not found")
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(ktest)]
mod tests {
    use super::*;
    use crate::vm::VMA_MANAGER;
    use ostd::prelude::ktest;

    /// Helper: initialise the VM subsystem once and return the global manager.
    fn vm() -> Arc<VmaManager> {
        crate::vm::init();
        VMA_MANAGER.get().unwrap().clone()
    }

    #[ktest]
    fn test_process_lifecycle() {
        // ── 1. Create init process ──────────────────────────────────────────
        let vm = vm();
        let init = Process::new(vm.clone(), "init");

        assert_eq!(init.name, "init");
        assert!(init.ppid.is_none());
        assert_eq!(init.state, ProcessState::Ready);

        // Verify it's in the global process table.
        assert!(PROCESS_TABLE.get_process(init.pid).is_some());

        // ── 2. Fork a child ─────────────────────────────────────────────────
        let mut child = init.fork().expect("fork failed");

        assert_ne!(child.pid, init.pid);
        assert_eq!(child.ppid, Some(init.pid));
        // Child inherits parent name until execve
        assert_eq!(child.name, "init");
        assert_eq!(init.children.lock().len(), 1);

        // ── 3. State transitions ────────────────────────────────────────────
        child.set_running();
        assert_eq!(child.state, ProcessState::Running);

        child.set_sleeping();
        assert_eq!(child.state, ProcessState::Sleeping);

        child.wake_up();
        assert_eq!(child.state, ProcessState::Ready);

        // ── 4. Child exits ──────────────────────────────────────────────────
        let child_pid = child.pid;
        child.exit(42);
        assert_eq!(child.state, ProcessState::Zombie);

        // ── 5. Parent reaps child via wait_child ────────────────────────────
        let result = init.wait_child(None);
        assert!(result.is_some());
        let (reaped_pid, exit_code) = result.unwrap();
        assert_eq!(reaped_pid, child_pid);
        assert_eq!(exit_code, 42);

        // Child must be removed from the process table after reaping.
        assert!(PROCESS_TABLE.get_process(child_pid).is_none());
        // And from the parent's child list.
        assert!(init.children.lock().is_empty());

        // ── 6. wait_child on empty list returns None ────────────────────────
        assert!(init.wait_child(None).is_none());
    }

    #[ktest]
    fn test_file_descriptors() {
        use crate::fs::ramfs::RamFs;
        use crate::fs::vfs::{init_root_fs, register_filesystem};

        // 1. Initialize filesystem if not already done
        let ramfs = Arc::new(RamFs);
        let _ = register_filesystem(ramfs);
        let _ = init_root_fs("ramfs", &[]);

        // 2. Create process
        let vm = vm();
        let proc = Process::new(vm, "test_fd_proc");

        // 3. Open a file for writing with O_CREAT
        // 0x40 is O_CREAT. Mode 0o644.
        let fd = proc
            .fd_table
            .lock()
            .open("/test_fd.txt", 0x40, 0o644)
            .expect("open failed");
        assert!(fd >= 0);

        // 4. Write some bytes
        let data = b"hello world";
        let written = proc
            .fd_table
            .lock()
            .write(fd, data)
            .expect("write failed");
        assert_eq!(written, data.len());

        // 5. Seek back to the beginning
        let offset = proc
            .fd_table
            .lock()
            .lseek(fd, 0, 0)
            .expect("lseek failed");
        assert_eq!(offset, 0);

        // 6. Read bytes back
        let mut buf = [0u8; 11];
        let read_len = proc
            .fd_table
            .lock()
            .read(fd, &mut buf)
            .expect("read failed");
        assert_eq!(read_len, 11);
        assert_eq!(&buf, data);

        // 7. Dup the file descriptor
        let fd2 = proc.fd_table.lock().dup(fd).expect("dup failed");
        assert_ne!(fd, fd2);

        // 8. Seek on first fd, read from second fd (shared offset test)
        let _ = proc
            .fd_table
            .lock()
            .lseek(fd, 6, 0)
            .expect("lseek failed");
        let mut buf2 = [0u8; 5];
        let read_len2 = proc
            .fd_table
            .lock()
            .read(fd2, &mut buf2)
            .expect("read failed");
        assert_eq!(read_len2, 5);
        assert_eq!(&buf2, b"world");

        // 9. Dup2 test
        let fd3 = proc.fd_table.lock().dup2(fd, 100).expect("dup2 failed");
        assert_eq!(fd3, 100);
        let offset3 = proc
            .fd_table
            .lock()
            .lseek(100, 0, 1)
            .expect("lseek current failed"); // seek current to verify offset is shared (should be 11)
        assert_eq!(offset3, 11);

        // 10. Close all
        proc.fd_table.lock().close(fd).expect("close fd failed");
        proc.fd_table.lock().close(fd2).expect("close fd2 failed");
        proc.fd_table.lock().close(fd3).expect("close fd3 failed");

        // 11. Verify they are closed
        assert!(proc.fd_table.lock().read(fd, &mut buf).is_err());
        assert!(proc.fd_table.lock().read(fd2, &mut buf).is_err());
        assert!(proc.fd_table.lock().read(fd3, &mut buf).is_err());
    }

    #[ktest]
    fn test_process_spawn_and_join_thread() {
        use alloc::sync::Arc;
        use core::sync::atomic::{AtomicBool, Ordering};

        let vm = vm();
        let proc = Process::new(vm, "thread-test");

        // Initially the process has no threads in its list.
        assert_eq!(proc.threads.lock().len(), 0);

        let ran = Arc::new(AtomicBool::new(false));
        let ran_clone = ran.clone();

        let thread = proc
            .spawn_thread("worker", move || {
                ran_clone.store(true, Ordering::Release);
            })
            .expect("spawn_thread failed");

        // The thread is now in the process's thread list.
        assert_eq!(proc.threads.lock().len(), 1);

        let tid = thread.tid();
        let exit_code = proc.join_thread(tid).expect("join_thread failed");

        // Thread ran and exited cleanly.
        assert!(ran.load(Ordering::Acquire));
        assert_eq!(exit_code, 0);

        // After joining, the thread is removed from the list.
        assert_eq!(proc.threads.lock().len(), 0);
    }

    #[ktest]
    fn test_process_multiple_threads() {
        use alloc::sync::Arc;
        use core::sync::atomic::{AtomicU32, Ordering};

        let vm = vm();
        let proc = Process::new(vm, "multi-thread-test");

        let counter = Arc::new(AtomicU32::new(0));
        let mut tids = alloc::vec::Vec::new();

        for _ in 0..4 {
            let c = counter.clone();
            let t = proc
                .spawn_thread("counter", move || {
                    c.fetch_add(1, Ordering::Relaxed);
                })
                .expect("spawn_thread failed");
            tids.push(t.tid());
        }

        assert_eq!(proc.threads.lock().len(), 4);

        for tid in tids {
            proc.join_thread(tid);
        }

        assert_eq!(counter.load(Ordering::Relaxed), 4);
        assert_eq!(proc.threads.lock().len(), 0);
    }
}
