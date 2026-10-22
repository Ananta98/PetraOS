// SPDX-License-Identifier: GPL-2.0

//! Process Control Block ([`Proc`]), mutable inner state, and process state types.
//!
//! [`Proc`] fields fall into three ownership classes: immutable identity that
//! all readers may use freely (`pid`), state behind a `SpinLock` whose guard
//! must not be held across blocking operations, and shared handles
//! (`Arc<...>`) that are cloned into child processes at fork with no deep copy.

use alloc::{collections::BTreeMap, string::String, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use ostd::sync::{SpinLock, WaitQueue};

use super::pid::{PID_INIT, Pid};
use crate::{
    fs::{PathNode, fd::Filedesc, vfs::File},
    proc::{
        signal::SigQueue,
        thread::{Thread, Tid},
    },
    uapi::{
        cred::Ucred,
        errno::Result,
        limit::{Plimit, Rusage},
        signal::{SigActs, SigInfo, Signal, code},
    },
    vm::vmar::Vmar,
};

/// Per-process I/O accounting statistics matching Linux `/proc/[pid]/io`.
#[derive(Debug, Default)]
pub struct ProcIoStats {
    /// Bytes read via read-like system calls.
    pub rchar: AtomicU64,
    /// Bytes written via write-like system calls.
    pub wchar: AtomicU64,
    /// Number of read system calls executed.
    pub syscr: AtomicU64,
    /// Number of write system calls executed.
    pub syscw: AtomicU64,
    /// Bytes transferred from storage.
    pub read_bytes: AtomicU64,
    /// Bytes transferred to storage.
    pub write_bytes: AtomicU64,
    /// Bytes of write operations cancelled/truncated.
    pub cancelled_write_bytes: AtomicU64,
}

impl ProcIoStats {
    /// Records one read-like syscall of `bytes`, incrementing the syscall
    /// counter and both the total-char and storage-read byte counters.
    ///
    /// Cheap enough for every I/O path: `Relaxed` atomics accept the coarse
    /// (non-exact) totals that this style of accounting provides.
    pub fn account_read(&self, bytes: usize) {
        self.syscr.fetch_add(1, Ordering::Relaxed);
        self.rchar.fetch_add(bytes as u64, Ordering::Relaxed);
        self.read_bytes.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    /// Records one write-like syscall of `bytes`, incrementing the syscall
    /// counter and both the total-char and storage-write byte counters.
    pub fn account_write(&self, bytes: usize) {
        self.syscw.fetch_add(1, Ordering::Relaxed);
        self.wchar.fetch_add(bytes as u64, Ordering::Relaxed);
        self.write_bytes.fetch_add(bytes as u64, Ordering::Relaxed);
    }
}

/// Maximum process command name length.
pub const COMM_LEN: usize = 16;

/// Encodes a process name into a fixed-length NUL-padded `comm` buffer,
/// mirroring Linux `task->comm` semantics.
///
/// Only the basename survives (`ps -o comm` convention), shrunk to
/// `COMM_LEN - 1` bytes so the buffer is always NUL-terminated.
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
    /// Encodes this status as the raw integer userspace passes to
    /// `waitpid`, decoded with the standard POSIX macros: `WEXITSTATUS`
    /// (bits 8-15, also used for stop signals), `WTERMSIG` (bits 0-6),
    /// and the core-dump flag (bit 7). The layout matches Linux so shells
    /// and libc wait helpers behave alike.
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

/// Mutable process state protected by [`Proc::inner`] `SpinLock`.
///
/// Guards must not be held across blocking operations; code that needs
/// multiple `inner` locks must always take them in a consistent order to
/// avoid deadlock.
pub struct ProcInner {
    /// Lifecycle state of the process.
    pub state: ProcState,
    /// Parent process; `None` for init or intentionally orphaned processes.
    pub ppid: Option<Pid>,
    /// Cached reaper chosen to inherit orphans from this process; resolved
    /// again via ancestor scan if unset when `exit` runs.
    pub reaper: Option<Pid>,
    /// PIDs of all direct children.
    pub children: Vec<Pid>,
    /// Threads that currently belong to this process.
    pub threads: Vec<Arc<Thread>>,
    /// Terminal status; populated when the process becomes a [`ProcState::Zombie`].
    pub xstat: Option<ExitStatus>,
    /// Fixed-length NUL-padded command name, see [`comm_from_name`].
    pub comm: [u8; COMM_LEN],
    /// Whether this subprocess serves as a reaper for orphans; currently
    /// true only for init (`PID_INIT`).
    pub is_reaper: bool,
    /// The process group this process belongs to.
    pub pgid: Option<Pid>,
    /// The session identifier this process belongs to.
    pub sid: Option<Pid>,
    /// Set while the process is stopped, and cleared when it resumes.
    pub stop_status: Option<ExitStatus>,
    /// System V shared memory attachments keyed by virtual start address.
    pub shm_attachments: BTreeMap<usize, crate::ipc::shm::ShmAttachment>,
    /// Executable path reported for this process. Seeded with the process
    /// name (a bare basename) when the process is first made, then replaced
    /// by `execve` with the full binary path it is actually running.
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
    /// Process-wide I/O accounting statistics.
    pub io_stats: ProcIoStats,
    /// Registered `membarrier(2)` commands, shared by every thread of the process.
    pub membarrier_state: AtomicU32,
    /// POSIX.1b interval timers (`timer_create(2)`).
    pub posix_timers: SpinLock<BTreeMap<i32, Arc<crate::time::posix_timer::PosixTimer>>>,
}

impl Proc {
    /// Creates a PCB for a freshly created process (fork or init start-up).
    ///
    /// The shared resources (`vmspace`, `fd_table`, `cred`, `limit`,
    /// `sigacts`) are handed over from the creator; fork callers clone them
    /// from the parent, `execve` uses [`set_vmspace`] instead of a new `Proc`.
    ///
    /// `comm_name` seeds both the [`comm_from_name`] buffer and
    /// [`ProcInner::exe_path`], which starts as the bare name until `execve`
    /// installs the real path.
    ///
    /// Only `PID_INIT` becomes [`ProcInner::is_reaper`], making init the
    /// default receiver for reparented orphan processes.
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
        let is_reaper = pid == PID_INIT;
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
            io_stats: ProcIoStats::default(),
            membarrier_state: AtomicU32::new(0),
            posix_timers: SpinLock::new(BTreeMap::new()),
        })
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

    /// Looks up the open file behind a descriptor.
    ///
    /// The single fd-lookup entry (one lock acquisition) for syscall
    /// handlers; bad descriptors fail with `EBADF`.
    pub fn file(&self, fd: i32) -> Result<Arc<File>> {
        self.fd_table.lock().get(fd)
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

    /// Atomically replaces the resource limit; readers observe either the
    /// old or the new limit, never a torn mix.
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
        let info = SigInfo::kernel(sig, code::KERNEL);
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
    /// The first resolution is cached so later calls just clone it.
    pub fn cwd(&self) -> Result<PathNode> {
        let mut slot = self.cwd.lock();
        if let Some(cached) = slot.clone() {
            return Ok(cached);
        }
        let root = crate::fs::root()?;
        *slot = Some(root.clone());
        Ok(root)
    }

    /// Sets the current working directory.
    pub fn set_cwd(&self, node: PathNode) {
        *self.cwd.lock() = Some(node);
    }

    /// Wakes any parent waiting on `vfork_waitq`.
    pub fn wake_vfork(&self) {
        self.vfork_waitq.wake_all();
    }

    /// Accounts for a read system call and bytes read.
    pub fn account_read(&self, bytes: usize) {
        self.io_stats.account_read(bytes);
    }

    /// Accounts for a write system call and bytes written.
    pub fn account_write(&self, bytes: usize) {
        self.io_stats.account_write(bytes);
    }
}
