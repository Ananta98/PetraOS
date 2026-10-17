// SPDX-License-Identifier: GPL-2.0

//! Process Control Block ([`Proc`]), mutable inner state, and process state types.

use alloc::{
    collections::BTreeMap,
    string::String,
    sync::Arc,
    vec::Vec,
};
use ostd::sync::{SpinLock, WaitQueue};

use super::pid::Pid;
use crate::{
    api::{
        cred::Ucred,
        errno::Result,
        limit::{Plimit, Rusage},
        signal::{SigActs, SigInfo, Signal},
    },
    fs::{PathNode, fd::Filedesc},
    proc::{
        signal::SigQueue,
        thread::{Thread, Tid},
    },
    vm::vmar::Vmar,
};

/// Maximum process command name length.
pub const COMM_LEN: usize = 16;

/// Encodes an executable or thread name into a fixed-length NUL-padded buffer.
pub fn comm_from_name(name: &str) -> [u8; COMM_LEN] {
    let mut buf = [0u8; COMM_LEN];
    let name = name.rsplit('/').next().unwrap_or(name);
    let bytes = name.as_bytes();
    let len = bytes.len().min(COMM_LEN - 1);
    buf[..len].copy_from_slice(&bytes[..len]);
    buf
}

/// Lifecycle state of a process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcState {
    /// Process is alive and actively running or sleeping.
    Alive,
    /// Process has terminated via `exit1` and is waiting for parent/reaper to reap.
    Zombie,
}

/// Process exit reason and status payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitStatus {
    /// Terminated normally via exit(code). Code is 0..=255.
    Exited(i32),
    /// Terminated abnormally due to an unhandled signal.
    Signaled { signal: Signal, core_dumped: bool },
    /// Stopped by a signal (e.g., SIGSTOP, SIGTSTP).
    Stopped(Signal),
    /// Continued by SIGCONT.
    Continued,
}

impl core::fmt::Display for ExitStatus {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Exited(code) => write!(f, "exit code {code}"),
            Self::Signaled {
                signal,
                core_dumped,
            } => {
                write!(
                    f,
                    "signal {} (core {})",
                    signal.as_u32(),
                    u8::from(*core_dumped)
                )
            }
            Self::Stopped(signal) => write!(f, "stopped by signal {}", signal.as_u32()),
            Self::Continued => f.write_str("continued"),
        }
    }
}

impl ExitStatus {
    /// Formats as standard POSIX wait status integer.
    pub fn as_raw(&self) -> i32 {
        match *self {
            ExitStatus::Exited(code) => (code & 0xff) << 8,
            ExitStatus::Signaled {
                signal,
                core_dumped,
            } => {
                let sig = signal.as_u32() as i32 & 0x7f;
                let core = if core_dumped { 0x80 } else { 0 };
                sig | core
            }
            ExitStatus::Stopped(signal) => ((signal.as_u32() as i32 & 0xff) << 8) | 0x7f,
            ExitStatus::Continued => 0xffff,
        }
    }
}

/// Mutable process state protected by `Proc::inner` SpinLock.
pub struct ProcInner {
    pub state: ProcState,
    pub ppid: Option<Pid>,
    pub reaper: Option<Pid>,
    pub children: Vec<Pid>,
    pub threads: Vec<Arc<Thread>>,
    pub xstat: Option<ExitStatus>,
    pub comm: [u8; COMM_LEN],
    pub is_reaper: bool,
    /// The process group this process belongs to.
    pub pgid: Option<Pid>,
    /// The session identifier this process belongs to.
    pub sid: Option<Pid>,
    /// Set while the process is stopped, and cleared when it resumes.
    pub stop_status: Option<ExitStatus>,
    /// System V shared memory attachments keyed by virtual start address.
    pub shm_attachments: BTreeMap<usize, crate::ipc::shm::ShmAttachment>,
    /// Full binary executable path.
    pub exe_path: Option<String>,
    /// Accumulated resource usage from reaped child processes.
    pub children_rusage: Rusage,
}

/// Process Control Block.
pub struct Proc {
    /// Unique process identifier. Immutable.
    pub pid: Pid,
    /// Mutable inner state.
    pub inner: SpinLock<ProcInner>,
    /// Address space descriptor. Replaced atomically during execve.
    pub vmspace: SpinLock<Arc<Vmar>>,
    /// Open file descriptor table. Shared across threads, cloned across fork.
    pub fd_table: Arc<SpinLock<Filedesc>>,
    /// Credentials.
    pub cred: SpinLock<Arc<Ucred>>,
    /// Per-process resource limits.
    pub limit: SpinLock<Arc<Plimit>>,
    /// Signal disposition table.
    pub sigacts: Arc<SpinLock<SigActs>>,
    /// Process-wide pending signal queue.
    pub sigqueue: SigQueue,
    /// Current working directory.
    pub cwd: SpinLock<Option<PathNode>>,
    /// WaitQueue for threads of this process stopped by job control signals.
    pub stop_queue: WaitQueue,
    /// WaitQueue for tasks waiting on child state changes.
    pub child_waitq: WaitQueue,
    /// WaitQueue for parents suspended in `vfork` awaiting child `execve` or `exit1`.
    pub vfork_waitq: WaitQueue,
}

impl Proc {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        pid: Pid,
        ppid: Option<Pid>,
        vmspace: Arc<Vmar>,
        fd_table: Arc<SpinLock<Filedesc>>,
        cred: Arc<Ucred>,
        limit: Arc<Plimit>,
        sigacts: Arc<SpinLock<SigActs>>,
        comm_name: &str,
    ) -> Arc<Self> {
        let is_reaper = pid == super::pid::PID_INIT;
        let comm = comm_from_name(comm_name);

        Arc::new(Self {
            pid,
            inner: SpinLock::new(ProcInner {
                state: ProcState::Alive,
                ppid,
                reaper: None,
                children: Vec::new(),
                threads: Vec::new(),
                xstat: None,
                comm,
                is_reaper,
                pgid: None,
                sid: None,
                stop_status: None,
                shm_attachments: BTreeMap::new(),
                exe_path: Some(String::from(comm_name)),
                children_rusage: Rusage::default(),
            }),
            vmspace: SpinLock::new(vmspace),
            fd_table,
            cred: SpinLock::new(cred),
            limit: SpinLock::new(limit),
            sigacts,
            sigqueue: SigQueue::new(),
            cwd: SpinLock::new(None),
            stop_queue: WaitQueue::new(),
            child_waitq: WaitQueue::new(),
            vfork_waitq: WaitQueue::new(),
        })
    }

    /// Returns the unique PID.
    pub fn pid(&self) -> Pid {
        self.pid
    }

    /// Returns process group identifier. Defaults to self.pid if not explicitly assigned.
    pub fn pgid(&self) -> Pid {
        self.inner.lock().pgid.unwrap_or(self.pid)
    }

    /// Returns parent process identifier if present.
    pub fn ppid(&self) -> Option<Pid> {
        self.inner.lock().ppid
    }

    /// Returns session identifier. Defaults to self.pid if not explicitly assigned.
    pub fn sid(&self) -> Pid {
        self.inner.lock().sid.unwrap_or(self.pid)
    }

    /// Returns lifecycle state.
    pub fn state(&self) -> ProcState {
        self.inner.lock().state
    }

    /// Returns active address space.
    pub fn vmspace(&self) -> Arc<Vmar> {
        Arc::clone(&*self.vmspace.lock())
    }

    /// Replaces address space.
    pub fn set_vmspace(&self, new_vmar: Arc<Vmar>) {
        *self.vmspace.lock() = new_vmar;
    }

    /// Returns current credentials.
    pub fn cred(&self) -> Arc<Ucred> {
        Arc::clone(&*self.cred.lock())
    }

    /// Sets credentials.
    pub fn set_cred(&self, new_cred: Arc<Ucred>) {
        *self.cred.lock() = new_cred;
    }

    /// Returns resource limits.
    pub fn limit(&self) -> Arc<Plimit> {
        Arc::clone(&*self.limit.lock())
    }

    /// Atomically replaces resource limits (Copy-on-Write).
    pub fn set_limit(&self, new_limit: Arc<Plimit>) {
        *self.limit.lock() = new_limit;
    }

    /// Adds a thread to this process.
    pub fn add_thread(&self, thread: Arc<Thread>) {
        let mut inner = self.inner.lock();
        inner.threads.push(thread);
    }

    /// Removes a thread from this process by TID.
    pub fn remove_thread(&self, tid: Tid) {
        let mut inner = self.inner.lock();
        inner.threads.retain(|td| td.tid != tid);
    }

    /// Returns the primary (first) thread of this process.
    pub fn main_thread(&self) -> Option<Arc<Thread>> {
        let inner = self.inner.lock();
        inner.threads.first().cloned()
    }

    /// Sends a signal to this process from kernel space.
    pub fn post_signal(&self, sig: Signal) {
        let info = SigInfo::kernel(sig, crate::api::signal::code::KERNEL);
        if let Err(err) = self.send_signal(info, 0) {
            ostd::debug!("proc: {sig} to pid {} was not delivered: {err}", self.pid);
        }
    }

    /// Returns printable command name.
    pub fn comm_name(&self) -> String {
        let inner = self.inner.lock();
        let len = inner
            .comm
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(inner.comm.len());
        String::from_utf8_lossy(&inner.comm[..len]).into_owned()
    }

    /// Returns the full binary executable path.
    pub fn exe_path(&self) -> String {
        self.inner
            .lock()
            .exe_path
            .clone()
            .unwrap_or_else(|| self.comm_name())
    }

    /// Returns current working directory, defaulting to VFS root if not yet set.
    pub fn cwd(&self) -> Result<PathNode> {
        let mut lock = self.cwd.lock();
        if let Some(ref node) = *lock {
            return Ok(node.clone());
        }
        let r = crate::fs::root()?;
        *lock = Some(r.clone());
        Ok(r)
    }

    /// Sets the current working directory.
    pub fn set_cwd(&self, node: PathNode) {
        *self.cwd.lock() = Some(node);
    }

    /// Wakes any parent waiting on `vfork_waitq`.
    pub fn wake_vfork(&self) {
        self.vfork_waitq.wake_all();
    }
}
