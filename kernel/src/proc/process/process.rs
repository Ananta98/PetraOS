use super::cmdline::CommandLine;
use super::pid::{next_pid, ProcessId};
use super::process_table::{register_process, unregister_process};
use crate::arch::paging::ArchPageTable;
use crate::ipc::signal::{MAX_SIGNALS, PendingSignals, SigAction};
use crate::mm::PageTable;
use crate::mm::vmm::AddrSpace;
use crate::proc::thread::{Thread, ThreadId, ThreadState};
use crate::sync::spinlock::Spinlock;
use alloc::collections::BTreeMap;
use crate::fs::FdTable;

use alloc::sync::Arc;
use alloc::vec::Vec;

/// Helper to read a binary executable file from VFS paths into memory.
pub fn read_file_from_vfs(path: &str) -> Result<Vec<u8>, &'static str> {
    let dentry = crate::fs::resolve_path(path).map_err(|_| "File not found in VFS")?;
    let stat = dentry.inode.ops.stat().map_err(|_| "Failed to stat file")?;
    let file_ops = dentry.inode.ops.open().map_err(|_| "Failed to open file ops")?;

    let alloc_size = if stat.size > 0 { stat.size as usize } else { 4096 };
    let mut buf = alloc::vec![0u8; alloc_size];
    let bytes_read = file_ops.read(0, &mut buf).map_err(|_| "Failed to read file data")?;
    buf.truncate(bytes_read);
    Ok(buf)
}

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
        Ok(Self {
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
            fd_table: Arc::new(crate::fs::FdTable::new()),
            heap_start: crate::arch::userspace::USER_HEAP_VBASE,
            heap_brk: crate::arch::userspace::USER_HEAP_VBASE,
            mmap_bump: crate::arch::userspace::USER_MMAP_VBASE,
        })
    }

    /// Execute an executable file with arguments and environment.
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

        log::info!(
            "Executing process '{}' (PID {}) with {} arg(s)",
            file_name,
            self.pid,
            cmdline.argc()
        );

        let binary_data = read_file_from_vfs(file_name)?;
        if binary_data.is_empty() {
            return Err("Executable file is empty");
        }

        // 1. Try loading as ELF binary
        if let Ok(elf) = crate::proc::loader::elf::Elf::new(&binary_data) {
            if let Ok(loaded_elf) = elf.load() {
                self.address_space = Arc::new(Spinlock::new(loaded_elf.addr_space));
                self.cmdline = cmdline;
                self.state = ProcessState::Running;
                return Ok((
                    loaded_elf.entry_point.as_u64(),
                    loaded_elf.stack_pointer.as_u64(),
                ));
            }
        }

        // 2. Fallback for raw binary payloads
        let mut addr_space_guard = self.address_space.lock();
        let addr_space = &mut *addr_space_guard;

        let code_vaddr = crate::mm::VirtAddr(crate::arch::userspace::USER_CODE_VBASE);
        let code_flags = crate::mm::MapFlags::READ
            | crate::mm::MapFlags::WRITE
            | crate::mm::MapFlags::EXECUTE
            | crate::mm::MapFlags::USER;

        addr_space
            .map_area(code_vaddr, 4096, code_flags, crate::mm::VmAreaKind::Anonymous)
            .map_err(|_| "Failed to map user code VMA")?;

        let code_phys = addr_space
            .page_table()
            .translate(code_vaddr)
            .ok_or("Failed to translate user code virtual page")?;

        let hhdm = crate::mm::hhdm_offset();
        let code_ptr = code_phys.as_ptr::<u8>(hhdm);
        let copy_len = core::cmp::min(binary_data.len(), 4096);

        // SAFETY: Copying binary content into physical frame.
        unsafe {
            core::ptr::copy_nonoverlapping(binary_data.as_ptr(), code_ptr, copy_len);
        }

        let stack_vaddr = crate::mm::VirtAddr(crate::arch::userspace::USER_STACK_VTOP - 4096);
        let stack_flags =
            crate::mm::MapFlags::READ | crate::mm::MapFlags::WRITE | crate::mm::MapFlags::USER;

        addr_space
            .map_area(stack_vaddr, 4096, stack_flags, crate::mm::VmAreaKind::Anonymous)
            .map_err(|_| "Failed to map user stack VMA")?;

        drop(addr_space_guard);

        self.cmdline = cmdline;
        self.state = ProcessState::Running;

        Ok((
            crate::arch::userspace::USER_CODE_VBASE,
            crate::arch::userspace::USER_STACK_VTOP,
        ))
    }

    /// Clone the current process (POSIX fork semantics).
    pub fn fork(parent: Arc<Spinlock<Process>>) -> Result<Arc<Spinlock<Process>>, &'static str> {
        let mut p_lock = parent.lock();
        let child_pid = next_pid();

        // 1. Copy-On-Write clone of virtual address space
        let child_addr_space = p_lock
            .address_space
            .lock()
            .clone()
            .map_err(|_| "Failed to clone address space for child process")?;

        let child_addr_space_arc = Arc::new(Spinlock::new(child_addr_space));
        let child_cr3 = child_addr_space_arc
            .lock()
            .page_table()
            .root()
            .as_u64() as usize;

        // 2. Initialize child process structure
        let mut child_proc = Process::new(child_pid, p_lock.pid)?;
        child_proc.pgid = p_lock.pgid;
        child_proc.address_space = child_addr_space_arc;
        child_proc.cmdline = p_lock.cmdline.clone();
        child_proc.sig_actions = p_lock.sig_actions;
        child_proc.fd_table = Arc::new(p_lock.fd_table.clone_table());
        child_proc.cwd = p_lock.cwd.clone();
        child_proc.heap_start = p_lock.heap_start;
        child_proc.heap_brk = p_lock.heap_brk;
        child_proc.mmap_bump = p_lock.mmap_bump;
        child_proc.state = p_lock.state;

        let child = Arc::new(Spinlock::new(child_proc));

        // 3. Clone process threads and register in scheduler
        let mut child_threads = BTreeMap::new();
        for (&_tid, thread_arc) in p_lock.threads.iter() {
            let t_lock = thread_arc.lock();
            let child_tid = crate::proc::thread::next_tid();

            let mut child_thread = Thread::new(
                child_tid,
                t_lock.name.clone(),
                t_lock.weight,
                Arc::downgrade(&child),
            );

            child_thread.context = t_lock.context;
            child_thread.context.cr3 = child_cr3;
            child_thread.sig_mask = t_lock.sig_mask;
            child_thread.state = t_lock.state;

            let c_thread_arc = Arc::new(Spinlock::new(child_thread));
            child_threads.insert(child_tid, c_thread_arc.clone());

            if t_lock.state == ThreadState::Ready || t_lock.state == ThreadState::Running {
                let saved_flags = crate::arch::disable_interrupts();
                crate::sched::SCHEDULER.lock().add_thread(c_thread_arc);
                if saved_flags {
                    crate::arch::enable_interrupts();
                }
            }
        }

        child.lock().threads = child_threads;

        register_process(child.clone());

        drop(p_lock);
        parent.lock().children.insert(child_pid, child.clone());

        Ok(child)
    }

    /// Wait for a child process to exit.
    pub fn wait(&mut self, pid: ProcessId) -> Result<i32, &'static str> {
        if !self.children.contains_key(&pid) {
            return Err("Child not found");
        }

        loop {
            let child = self.children.get(&pid).unwrap().clone();
            let c_lock = child.lock();
            if c_lock.state == ProcessState::Zombie {
                let exit_code = c_lock.exit_code.unwrap_or(0);
                drop(c_lock);
                self.children.remove(&pid);
                unregister_process(pid);
                return Ok(exit_code);
            }
            drop(c_lock);

            Thread::yield_cpu();
        }
    }

    /// Terminate the process.
    pub fn exit(&mut self, status: i32) {
        self.state = ProcessState::Zombie;
        self.exit_code = Some(status);

        // Terminate all threads
        let saved_flags = crate::arch::disable_interrupts();
        for (_, thread) in self.threads.iter_mut() {
            let mut t_lock = thread.lock();
            t_lock.state = ThreadState::Zombie;
            crate::sched::SCHEDULER.lock().remove_thread(t_lock.tid);
        }
        if saved_flags {
            crate::arch::enable_interrupts();
        }
        self.threads.clear();
    }

    /// Update signal action for a given signal number (sigaction semantics).
    pub fn sigaction(&mut self, sig: u8, act: Option<SigAction>) -> Result<SigAction, &'static str> {
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
    pub fn handle_pending_signals(
        &mut self,
        thread: &mut Thread,
        frame: &mut crate::arch::syscall::syscall::SyscallFrame,
    ) {
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