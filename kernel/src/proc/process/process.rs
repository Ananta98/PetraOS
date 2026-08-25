use super::cmdline::CommandLine;
use super::credentials::Credentials;
use super::pid::{ProcessId, next_pid};
use super::process_table::{register_process, unregister_process};
use crate::arch::syscall::syscall::SyscallFrame;
use crate::arch::userspace;
use crate::fs::FdTable;
use crate::ipc::signal::{MAX_SIGNALS, PendingSignals, SigAction};
use crate::mm::ArchPageTable;
use crate::mm::PageTable;
use crate::mm::vmm::AddrSpace;
use crate::mm::{PageTableFlags, VirtAddr};
use crate::proc::loader::elf::Elf;
use crate::proc::thread::{Thread, ThreadId, ThreadState};
use crate::sync::spinlock::Spinlock;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;

/// State of a process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    Creating,
    Ready,
    Running,
    Stopped,
    Zombie,
}

/// Represents a Process (Task Group) containing shared resources.
pub struct Process {
    /// Process ID (PID)
    pub pid: ProcessId,

    /// Parent Process ID (PPID)
    pub ppid: ProcessId,

    /// Process Group ID (PGID)
    pub pgid: ProcessId,

    /// Current working directory
    pub cwd: alloc::string::String,

    /// Process state
    pub state: ProcessState,

    /// Virtual Address Space
    pub address_space: Arc<Spinlock<AddrSpace<ArchPageTable>>>,

    /// Command line arguments and environment variables
    pub cmdline: CommandLine,

    /// Exit code when process terminates
    pub exit_code: Option<i32>,

    /// Signal actions (handlers) for the process
    pub sig_actions: [SigAction; MAX_SIGNALS],

    /// Pending signals directed to the whole process
    pub pending_signals: PendingSignals,

    /// Children processes list
    pub children: BTreeMap<ProcessId, Arc<Spinlock<Process>>>,

    /// Threads running in this process
    pub threads: BTreeMap<ThreadId, Arc<Spinlock<Thread>>>,

    /// Process credentials (uid, gid, euid, egid, etc.)
    pub creds: Arc<Credentials>,

    /// File mode creation mask (umask)
    pub umask: u32,

    /// Per-process file descriptor table
    pub fd_table: Arc<FdTable>,

    /// Virtual memory heap break start (for brk syscall)
    pub heap_start: u64,

    /// Current virtual memory heap break address
    pub heap_brk: u64,

    /// Next virtual address for mmap allocation
    pub mmap_bump: u64,
}

impl Process {
    pub fn new(pid: ProcessId, ppid: ProcessId) -> Result<Self, &'static str> {
        let page_table =
            ArchPageTable::new().map_err(|_| "Failed to allocate process page table")?;
        let address_space = Arc::new(Spinlock::new(AddrSpace::new(page_table)));
        Ok(Self::new_with_address_space(pid, ppid, address_space))
    }

    /// Creates a new `Process` instance with an existing address space (e.g. for `fork`).
    pub fn new_with_address_space(
        pid: ProcessId,
        ppid: ProcessId,
        address_space: Arc<Spinlock<AddrSpace<ArchPageTable>>>,
    ) -> Self {
        Self {
            pid,
            ppid,
            pgid: pid,
            cwd: alloc::string::String::from("/"),
            state: ProcessState::Creating,
            address_space,
            cmdline: CommandLine::default(),
            exit_code: None,
            sig_actions: [Default::default(); MAX_SIGNALS],
            pending_signals: PendingSignals::new(),
            children: BTreeMap::new(),
            threads: BTreeMap::new(),
            creds: super::credentials::Credentials::new(),
            umask: 0o022,
            fd_table: Arc::new(crate::fs::FdTable::new()),
            heap_start: userspace::USER_HEAP_VBASE,
            heap_brk: userspace::USER_HEAP_VBASE,
            mmap_bump: userspace::USER_MMAP_VBASE,
        }
    }

    /// Execute an executable file with raw argument and environment pointers.
    pub fn execute(
        &mut self,
        file_name: &str,
        argc: usize,
        argv: *const *const u8,
        envp: *const *const u8,
    ) -> Result<(u64, u64), &'static str> {
        let cmdline = if !argv.is_null() {
            unsafe { CommandLine::from_raw(argc, argv, envp)? }
        } else {
            CommandLine::default()
        };
        self.execute_cmdline(file_name, cmdline)
    }

    /// Execute an executable file with a structured `CommandLine` (argv + envp).
    pub fn execute_cmdline(
        &mut self,
        file_name: &str,
        cmdline: CommandLine,
    ) -> Result<(u64, u64), &'static str> {
        log::info!(
            "Executing process '{}' (PID {}) with {} arg(s) and {} env var(s)",
            file_name,
            self.pid,
            cmdline.argc(),
            cmdline.envp().len()
        );

        let binary_data =
            crate::fs::read_file(file_name).map_err(|_| "Failed to read binary from VFS")?;
        if binary_data.is_empty() {
            return Err("Executable file is empty");
        }

        // 1. Shebang (`#!`) script interpreter support
        if binary_data.starts_with(b"#!") {
            let first_line_end = binary_data
                .iter()
                .position(|&b| b == b'\n')
                .unwrap_or(binary_data.len());
            if let Ok(line_str) = core::str::from_utf8(&binary_data[2..first_line_end]) {
                let trimmed = line_str.trim();
                let mut parts = trimmed.split_whitespace();
                if let Some(interpreter) = parts.next() {
                    let mut new_args = alloc::vec::Vec::new();
                    new_args.push(alloc::string::String::from(interpreter));
                    if let Some(arg) = parts.next() {
                        new_args.push(alloc::string::String::from(arg));
                    }
                    new_args.push(alloc::string::String::from(file_name));
                    if cmdline.argc() > 1 {
                        for arg in &cmdline.args[1..] {
                            new_args.push(arg.clone());
                        }
                    }
                    let new_cmdline = CommandLine::new(new_args, cmdline.env.clone());
                    return self.execute_cmdline(interpreter, new_cmdline);
                }
            }
        }

        // 2. Close-on-exec (FD_CLOEXEC) descriptor cleanup
        self.fd_table.close_on_exec();

        // 3. Reset non-ignored signals to default handlers
        for action in self.sig_actions.iter_mut() {
            if action.handler != crate::ipc::signal::SIG_IGN {
                *action = Default::default();
            }
        }

        // 4. Load ELF binary
        let elf = Elf::new(&binary_data).map_err(|err| {
            log::error!("[Process] ELF parsing failed for '{}': {}", file_name, err);
            "ELF parsing failed"
        })?;

        let loaded_elf = elf
            .load_with_cmdline(Some(&cmdline), Some(&self.creds))
            .map_err(|err| {
                log::error!("[Process] ELF loading failed for '{}': {}", file_name, err);
                "ELF loading failed"
            })?;

        self.address_space = Arc::new(Spinlock::new(loaded_elf.addr_space));
        self.cmdline = cmdline;
        self.heap_start = userspace::USER_HEAP_VBASE;
        self.heap_brk = userspace::USER_HEAP_VBASE;
        self.mmap_bump = userspace::USER_MMAP_VBASE;
        self.state = ProcessState::Running;

        Ok((
            loaded_elf.entry_point.as_u64(),
            loaded_elf.stack_pointer.as_u64(),
        ))
    }

    /// Fork a child process duplicating this process (POSIX fork).
    pub fn fork(
        parent: Arc<Spinlock<Process>>,
        parent_frame: &crate::arch::syscall::SyscallFrame,
    ) -> Result<Arc<Spinlock<Process>>, &'static str> {
        let mut p_lock = parent.lock();
        let child_pid = next_pid();

        // 1. Copy-On-Write clone of virtual address space
        let child_addr_space = p_lock
            .address_space
            .lock()
            .clone()
            .map_err(|_| "Failed to clone address space for child process")?;

        let child_addr_space_arc = Arc::new(Spinlock::new(child_addr_space));
        let child_cr3 = child_addr_space_arc.lock().page_table().root().as_u64() as usize;

        // 2. Initialize child process structure without redundant page table allocation
        let mut child_proc =
            Process::new_with_address_space(child_pid, p_lock.pid, child_addr_space_arc);
        child_proc.pgid = p_lock.pgid;
        child_proc.cmdline = p_lock.cmdline.clone();
        child_proc.sig_actions = p_lock.sig_actions;
        child_proc.fd_table = Arc::new(p_lock.fd_table.clone_table());
        child_proc.cwd = p_lock.cwd.clone();
        child_proc.creds = Arc::clone(&p_lock.creds);
        child_proc.umask = p_lock.umask;
        child_proc.heap_start = p_lock.heap_start;
        child_proc.heap_brk = p_lock.heap_brk;
        child_proc.mmap_bump = p_lock.mmap_bump;
        child_proc.state = p_lock.state;

        let child = Arc::new(Spinlock::new(child_proc));

        // 3. Create child primary thread replicating the calling thread's context and user register state
        let mut child_threads = BTreeMap::new();
        let child_tid = crate::proc::thread::next_tid();

        let (thread_name, thread_weight, sig_mask, fs_base, gs_base) = crate::proc::current_thread()
            .or_else(|| p_lock.threads.values().next().cloned())
            .map(|t| {
                let t_lock = t.lock();
                (
                    t_lock.name.clone(),
                    t_lock.weight,
                    t_lock.sig_mask,
                    t_lock.context.fs_base,
                    t_lock.context.gs_base,
                )
            })
            .unwrap_or_else(|| (alloc::string::String::from("fork_child"), 1024, 0, 0, 0));

        let mut child_thread = Thread::new(
            child_tid,
            thread_name,
            thread_weight,
            Arc::downgrade(&child),
        );

        let mut child_kstack = crate::arch::cpu::stack::KernelStack::new()
            .map_err(|_| "Failed to allocate kernel stack for child process")?;
        let child_rsp = crate::arch::cpu::stack::init_fork_stack(&mut child_kstack, parent_frame);

        child_thread.setup_fork_context(
            child_kstack,
            child_rsp,
            child_cr3,
            fs_base,
            gs_base,
            sig_mask,
        );

        let c_thread_arc = Arc::new(Spinlock::new(child_thread));
        child_threads.insert(child_tid, c_thread_arc.clone());

        crate::sched::add_thread(c_thread_arc);

        child.lock().threads = child_threads;

        register_process(child.clone());
        p_lock.children.insert(child_pid, child.clone());
        drop(p_lock);

        Ok(child)
    }

    /// Attempt a single non-blocking check for a child process state change (POSIX wait4).
    ///
    /// Returns:
    /// - `Ok(Some((pid, status)))` if a matching child process transitioned to Zombie or Stopped.
    /// - `Ok(None)` if matching children exist but none have changed state yet.
    /// - `Err(SyscallError::ECHILD)` if no matching children exist.
    pub fn try_wait4(
        &mut self,
        pid_req: i32,
        wuntraced: bool,
    ) -> Result<Option<(ProcessId, i32)>, crate::syscalls::SyscallError> {
        let mut has_matching_children = false;
        let mut found_pid = None;
        let mut found_status = 0;

        for (&child_pid, child_arc) in self.children.iter() {
            let c_lock = child_arc.lock();
            let matches = if pid_req == -1 {
                true
            } else if pid_req > 0 {
                child_pid.as_u64() == pid_req as u64
            } else if pid_req == 0 {
                c_lock.pgid == self.pgid
            } else {
                c_lock.pgid.as_u64() == (-pid_req) as u64
            };

            if !matches {
                continue;
            }

            has_matching_children = true;

            if c_lock.state == ProcessState::Zombie {
                let code = c_lock.exit_code.unwrap_or(0);
                found_pid = Some(child_pid);
                found_status = (code & 0xFF) << 8;
                break;
            } else if c_lock.state == ProcessState::Stopped && wuntraced {
                let sig = 19; // SIGSTOP
                found_pid = Some(child_pid);
                found_status = (sig << 8) | 0x7F;
                break;
            }
        }

        if let Some(child_pid) = found_pid {
            self.children.remove(&child_pid);
            unregister_process(child_pid);
            return Ok(Some((child_pid, found_status)));
        }

        if !has_matching_children {
            return Err(crate::syscalls::SyscallError::ECHILD);
        }

        Ok(None)
    }

    /// Wait for a child process state change (POSIX wait4).
    pub fn wait4(
        &mut self,
        pid_req: i32,
        options: i32,
    ) -> Result<(ProcessId, i32), crate::syscalls::SyscallError> {
        let wnohang = (options & 1) != 0;
        let wuntraced = (options & 2) != 0;

        if let Some(res) = self.try_wait4(pid_req, wuntraced)? {
            return Ok(res);
        }

        if wnohang {
            return Ok((ProcessId(0), 0));
        }

        Err(crate::syscalls::SyscallError::ECHILD)
    }

    /// Wait for a child process to exit.
    pub fn wait(&mut self, pid: ProcessId) -> Result<i32, &'static str> {
        match self.try_wait4(pid.as_u64() as i32, false) {
            Ok(Some((_, status))) => Ok((status >> 8) & 0xFF),
            _ => Err("Child not found or wait failed"),
        }
    }

    /// Terminate the process.
    ///
    /// Marks the process zombie and removes all **queued** (non-current) threads from the
    /// scheduler run queue. The currently-executing thread is in `current_threads[cpu_id]`,
    /// not the run queue, so it must NOT be removed here; `schedule(false)` → `block_current()`
    /// will clear it after this function returns.
    pub fn exit(&mut self, status: i32) {
        self.state = ProcessState::Zombie;
        self.exit_code = Some(status);

        // Detach all shared memory segments for this process
        crate::ipc::shm::SHM_MANAGER.lock().on_process_exit(self.pid.as_u64() as u32);

        // Determine the TID of the currently-running thread on this CPU.
        let cpu_id = crate::arch::cpu_id();
        let current_tid = crate::sched::current_thread_on_cpu(cpu_id)
            .as_ref()
            .map(|t| t.lock().tid);

        // Remove all non-current threads from the scheduler run queue.
        for (_, thread) in self.threads.iter() {
            let mut t_lock = thread.lock();
            let tid = t_lock.tid;
            let is_current = current_tid.map_or(false, |ctid| ctid == tid);
            t_lock.state = ThreadState::Zombie;
            drop(t_lock);
            if !is_current {
                crate::sched::remove_thread(tid);
            }
        }
    }

    /// Update signal action for a given signal number (sigaction semantics).
    pub fn sigaction(
        &mut self,
        sig: u8,
        act: Option<SigAction>,
    ) -> Result<SigAction, &'static str> {
        if sig == 0 || sig > 64 {
            return Err("Invalid signal number");
        }
        if crate::ipc::signal::is_uncatchable(sig) && act.is_some() {
            return Err("Cannot catch or ignore SIGKILL or SIGSTOP");
        }

        let sig_idx = (sig - 1) as usize;
        let old_action = self.sig_actions[sig_idx];
        if let Some(new_action) = act {
            self.sig_actions[sig_idx] = new_action;
        }

        Ok(old_action)
    }

    /// Send a POSIX signal to this process.
    pub fn send_signal(&mut self, sig: u8) -> Result<(), &'static str> {
        if sig == 0 || sig > 64 {
            return Err("Invalid signal number");
        }

        // Special immediate signals
        if sig == crate::ipc::signal::SIGKILL {
            log::info!("Process PID {} terminated by SIGKILL", self.pid);
            self.exit(128 + sig as i32);
            return Ok(());
        }

        if sig == crate::ipc::signal::SIGSTOP {
            self.state = ProcessState::Stopped;
            for (_, thread_arc) in self.threads.iter() {
                let mut t = thread_arc.lock();
                t.state = ThreadState::Stopped;
            }
            return Ok(());
        }

        if sig == crate::ipc::signal::SIGCONT {
            if self.state == ProcessState::Stopped {
                self.state = ProcessState::Running;
            }
            for (_, thread_arc) in self.threads.iter() {
                let mut t = thread_arc.lock();
                if t.state == ThreadState::Stopped {
                    t.state = ThreadState::Ready;
                }
            }
            return Ok(());
        }

        // Standard signal delivery: add to process pending queue
        self.pending_signals.add(sig);

        // Wake up sleeping threads so they can process the signal (UNIX EINTR behavior)
        for (_, thread_arc) in self.threads.iter() {
            Thread::unblock(thread_arc.clone());
        }

        Ok(())
    }

    /// Evaluate and handle pending signals for a process thread prior to user return.
    pub fn handle_pending_signals(&mut self, thread: &mut Thread, frame: &mut SyscallFrame) {
        let sig_opt = self
            .pending_signals
            .dequeue(thread.sig_mask)
            .or_else(|| thread.pending_signals.dequeue(thread.sig_mask));

        let sig = match sig_opt {
            Some(s) => s,
            None => return,
        };

        let action = self.sig_actions[(sig - 1) as usize];

        if action.handler == crate::ipc::signal::SIG_IGN {
            return;
        }

        if action.handler == crate::ipc::signal::SIG_DFL {
            match crate::ipc::signal::default_action(sig) {
                crate::ipc::signal::SignalDefaultAction::Terminate
                | crate::ipc::signal::SignalDefaultAction::CoreDump => {
                    log::info!("Process PID {} killed by signal {}", self.pid, sig);
                    self.exit(128 + sig as i32);
                }
                crate::ipc::signal::SignalDefaultAction::Stop => {
                    self.state = ProcessState::Stopped;
                    thread.state = ThreadState::Stopped;
                }
                crate::ipc::signal::SignalDefaultAction::Continue => {
                    self.state = ProcessState::Running;
                    thread.state = ThreadState::Running;
                }
                crate::ipc::signal::SignalDefaultAction::Ignore => {}
            }
            return;
        }

        // Custom signal handler execution frame setup
        let old_mask = thread.sig_mask;
        thread.sig_mask |= action.mask | (1 << (sig - 1));

        unsafe {
            let _ = crate::arch::signal::setup_signal_frame(frame, sig, &action, old_mask);
        }
    }
}
