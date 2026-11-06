// SPDX-License-Identifier: GPL-2.0

//! POSIX message-queue state (`mq_overview(7)`).
//!
//! A queue is an independent named object, keyed by name in [`MQ_REGISTRY`].
//! `mq_open(2)` hands out a per-open *descriptor* ([`MqDesc`], the kernel-side
//! `mqd_t`) over one queue; several descriptors may reference the same queue.
//! The queue object outlives any one descriptor: it is freed once unlinked
//! and the last descriptor closes.
//!
//! Attributes userspace picks are validated against Linux's own limits: at
//! most [`MQ_MAX_MSG_COUNT`] messages per queue, [`MQ_MAX_MSGSIZE`] bytes per
//! message, and [`MQ_MAX_QUEUE_BYTES`] bytes pending per queue.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::time::Duration;
use ostd::sync::{SpinLock, WaitQueue};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::fs::poll::PollEvents;
use crate::fs::vfs::{File, FileOps, INode, INodeAttr, Mode, NodeOps, OpenFlags, SeekAnchor};
use crate::proc::{allproc_find, Pid};
use crate::proc::thread::Thread;
use crate::uapi::errno::{Errno, Result};
use crate::uapi::signal::SigInfo;

/// The default per-queue message limit (`/proc/sys/fs/mqueue/msg_default`).
pub const DEFAULT_MAXMSG: usize = 10;
/// The default message size (`/proc/sys/fs/mqueue/msgsize_default`).
pub const DEFAULT_MSGSIZE: usize = 8192;
/// Hard cap on `mq_msgsize` userspace may pick (`msgsize_max`).
pub const MQ_MAX_MSGSIZE: usize = 1 << 20;
/// Hard cap on `mq_maxmsg` userspace may pick (`msg_max`).
pub const MQ_MAX_MSG_COUNT: usize = 65535;
/// Hard cap on the bytes one queue may hold.
pub const MQ_MAX_QUEUE_BYTES: usize = 8 << 20;
/// POSIX `MQ_PRIO_MAX - 1`.
pub const MQ_PRIO_MAX: u32 = 32767;
/// Maximum queue-name length.
pub const MQ_NAME_MAX: usize = 255;
/// open(2)-style bits `mq_open(2)` recognizes, alongside [`OpenFlags`].
pub const O_CREAT: i32 = 0o100;
pub const O_EXCL: i32 = 0o200;
pub const O_NONBLOCK: i32 = 0o4000;
pub const O_CLOEXEC: i32 = 0o2000000;

/// `siginfo_t.si_code` for a `mq_notify(2)` signal (`SI_MESGQ`).
const SI_MESGQ: i32 = -3;

/// Queues are process-independent; this registry is their only shared state.
static MQ_REGISTRY: SpinLock<BTreeMap<String, Arc<Mqueue>>> = SpinLock::new(BTreeMap::new());

/// One queued message.
struct MqMsg {
    prio: u32,
    data: Vec<u8>,
}

/// The registered `mq_notify(2)` state, one registration at a time per queue.
struct NotifyState {
    /// Process that is the target of the next message-arrival signal.
    owner_pid: u32,
    signo: i32,
    value: u64,
}

/// The shared core of one POSIX message queue.
pub struct Mqueue {
    /// POSIX name, including its leading `/`.
    pub name: String,
    maxmsg: usize,
    msgsize: usize,
    /// Creation `mode_t`, kept so later opens can permission-check.
    pub mode: u32,
    /// Creator's uid, for permission checks.
    pub owner_uid: u32,
    /// Creator's gid, for permission checks.
    pub owner_gid: u32,
    /// Pending messages, owned by the queue.
    msgs: SpinLock<VecDeque<MqMsg>>,
    /// Total bytes currently queued, a fast capacity check next to `maxmsg`.
    bytes: SpinLock<usize>,
    /// Waiters blocked on space (senders); woken by receives and timers.
    send_wait: WaitQueue,
    /// Waiters blocked on emptiness (receivers); woken by sends and timers.
    recv_wait: WaitQueue,
    notify: SpinLock<Option<NotifyState>>,
}

impl Mqueue {
    /// Byte capacity this queue never exceeds: `mq_maxmsg * mq_msgsize`.
    pub const fn capacity(&self) -> usize {
        self.maxmsg * self.msgsize
    }

    /// Returns an existing queue by full POSIX name, if registered.
    pub fn find(name: &str) -> Option<Arc<Mqueue>> {
        MQ_REGISTRY.lock().get(name).cloned()
    }
}

/// Removes a queue from the registry (`mq_unlink(2)`).
///
/// Live descriptors keep working; the object is freed when the last closes.
pub fn queue_unlink(name: &str) -> Result<()> {
    match MQ_REGISTRY.lock().remove(name) {
        Some(_) => Ok(()),
        None => Err(Errno::ENOENT),
    }
}

/// Tests an open request against the queue's creation mode bits, following
/// open(2)'s owner / group / other reduction.
fn access_permits(queue: &Mqueue, wants_write: bool) -> Result<()> {
    let cred = Thread::proc_or_esrch()?.cred();
    if cred.cr_uid.is_root() {
        return Ok(());
    }
    let uid = cred.cr_uid.0;
    let gid = cred.cr_gid.0;
    let bits = if uid == queue.owner_uid {
        (queue.mode >> 6) & 0o7
    } else if gid == queue.owner_gid {
        (queue.mode >> 3) & 0o7
    } else {
        queue.mode & 0o7
    };
    let needed = if wants_write { 0o2 } else { 0o4 };
    if bits & needed != 0 {
        Ok(())
    } else {
        Err(Errno::EACCES)
    }
}

/// Opens (or creates and registers) a queue.
///
/// Returns `(queue, freshly-created)`. When `use_attrs` is set the supplied
/// attribute pair overrides the defaults; the pair is range-checked first.
pub fn queue_open(
    name: &str,
    oflags: i32,
    mode: u32,
    use_attrs: bool,
    attr_maxmsg: usize,
    attr_msgsize: usize,
) -> Result<(Arc<Mqueue>, bool)> {
    if name.is_empty() || name.len() > MQ_NAME_MAX {
        return Err(Errno::EINVAL);
    }

    let create = (oflags & O_CREAT) != 0;
    let wants_write = (oflags & 1) != 0;
    if let Some(queue) = Mqueue::find(name) {
        if create && (oflags & O_EXCL) != 0 {
            return Err(Errno::EEXIST);
        }
        access_permits(&queue, wants_write)?;
        return Ok((queue, false));
    }
    if !create {
        return Err(Errno::ENOENT);
    }

    let (maxmsg, msgsize) = if use_attrs {
        (attr_maxmsg, attr_msgsize)
    } else {
        (DEFAULT_MAXMSG, DEFAULT_MSGSIZE)
    };
    if maxmsg == 0
        || maxmsg > MQ_MAX_MSG_COUNT
        || msgsize == 0
        || msgsize > MQ_MAX_MSGSIZE
        || maxmsg.saturating_mul(msgsize) > MQ_MAX_QUEUE_BYTES
    {
        return Err(Errno::EINVAL);
    }

    let cred = Thread::proc_or_esrch()?.cred().clone();
    let queue = Arc::new(Mqueue {
        name: String::from(name),
        maxmsg,
        msgsize,
        mode: mode & 0o7777,
        owner_uid: cred.cr_uid.0,
        owner_gid: cred.cr_gid.0,
        msgs: SpinLock::new(VecDeque::new()),
        bytes: SpinLock::new(0),
        send_wait: WaitQueue::new(),
        recv_wait: WaitQueue::new(),
        notify: SpinLock::new(None),
    });
    MQ_REGISTRY.lock().insert(String::from(name), queue.clone());
    Ok((queue, true))
}

/// Attempts one send; `None` means "queue full", so the caller may block.
fn try_send(queue: &Mqueue, slot: &mut Option<Vec<u8>>, prio: u32) -> Option<Result<usize>> {
    let payload_len = match slot.as_ref() {
        Some(data) => data.len(),
        None => return Some(Err(Errno::EFAULT)),
    };
    if *queue.bytes.lock() + payload_len > queue.capacity()
        || queue.msgs.lock().len() >= queue.maxmsg
    {
        return None;
    }

    let data = slot.take()?;
    *queue.bytes.lock() = queue.bytes.lock().saturating_add(payload_len);
    queue.msgs.lock().push_back(MqMsg { prio, data });

    queue.recv_wait.wake_all();
    crate::fs::poll::notify();
    // A fresh message satisfies exactly one registered notifier, POSIX-style.
    deliver_notify(queue);
    Some(Ok(payload_len))
}

/// Attempts one receive; `None` means "queue empty", so the caller may block.
///
/// The highest-priority message wins, FIFO within one priority, matching
/// `mq_receive(3p)`. Returns `(bytes_copied, priority)`.
fn try_recv(queue: &Mqueue, buf: &mut [u8]) -> Option<Result<(usize, u32)>> {
    let mut msgs = queue.msgs.lock();
    if msgs.is_empty() {
        return None;
    }
    let mut best = 0usize;
    for i in 1..msgs.len() {
        if msgs[i].prio > msgs[best].prio {
            best = i;
        }
    }
    let msg = msgs.get(best)?;
    let data_len = msg.data.len();
    let (copied, prio) = {
        let copied = core::cmp::min(data_len, buf.len());
        buf[..copied].copy_from_slice(&msg.data[..copied]);
        (copied, msg.prio)
    };
    msgs.remove(best);
    drop(msgs);

    *queue.bytes.lock() = queue.bytes.lock().saturating_sub(data_len);
    queue.send_wait.wake_all();
    crate::fs::poll::notify();
    Some(Ok((copied, prio)))
}

/// Fires the one registered notification, if any; POSIX clears it after use.
fn deliver_notify(queue: &Mqueue) {
    let state = queue.notify.lock().take();
    let Some(state) = state else {
        return;
    };
    let Some(pid) = Pid::from_u32(state.owner_pid) else {
        return;
    };
    if let Some(target) = allproc_find(pid) {
        let _ = target.send_signal(
            SigInfo {
                si_signo: state.signo,
                si_code: SI_MESGQ,
                si_value: state.value,
                si_pid: state.owner_pid as i32,
                ..SigInfo::default()
            },
            state.owner_pid,
        );
        target.child_waitq.wake_all();
    }
}

/// Registers or removes a notification on a queue (`mq_notify(2)`).
///
/// `signo == 0` removes the caller's own registration or reports a foreign
/// one with `EPERM`; replacing a foreign notification is `EBUSY`.
pub fn mq_register_notify(queue: &Mqueue, signo: i32, value: u64) -> Result<()> {
    let pid = Thread::current_proc()
        .map(|p| p.pid.as_u32())
        .ok_or(Errno::ESRCH)?;

    let mut notify = queue.notify.lock();
    if let Some(existing) = notify.as_ref() {
        if existing.owner_pid != pid {
            return if signo == 0 {
                Err(Errno::EPERM)
            } else {
                Err(Errno::EBUSY)
            };
        }
    }
    *notify = if signo == 0 {
        None
    } else {
        Some(NotifyState {
            owner_pid: pid,
            signo,
            value,
        })
    };
    Ok(())
}

/// One open message-queue descriptor (`mqd_t`).
pub struct MqDesc {
    pub queue: Arc<Mqueue>,
    nonblocking: SpinLock<bool>,
}

/// The attribute tuple `struct mq_attr` exposes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct MqAttrSnapshot {
    /// `mq_flags`: only `O_NONBLOCK` is settable, per descriptor.
    pub flags: u64,
    /// `mq_maxmsg`.
    pub maxmsg: u64,
    /// `mq_msgsize`.
    pub msgsize: u64,
    /// `mq_curmsgs`.
    pub curmsgs: u64,
}

impl MqDesc {
    /// Creates a descriptor over `queue` with an O_NONBLOCK state applied.
    pub fn new(queue: Arc<Mqueue>, nonblocking: bool) -> Self {
        Self {
            queue,
            nonblocking: SpinLock::new(nonblocking),
        }
    }

    pub fn nonblocking(&self) -> bool {
        *self.nonblocking.lock()
    }

    pub fn set_nonblocking(&self, on: bool) {
        *self.nonblocking.lock() = on;
    }

    /// Snapshot for `mq_getsetattr(2)`'s old-attribute write-back.
    pub fn attr_snapshot(&self) -> MqAttrSnapshot {
        MqAttrSnapshot {
            flags: if self.nonblocking() { O_NONBLOCK as u64 } else { 0 },
            maxmsg: self.queue.maxmsg as u64,
            msgsize: self.queue.msgsize as u64,
            curmsgs: self.queue.msgs.lock().len() as u64,
        }
    }

    /// `mq_send(3p)` / `mq_timedsend(3p)`; `timeout_ns` is relative.
    pub fn send(&self, payload: Vec<u8>, prio: u32, timeout_ns: Option<u64>) -> Result<usize> {
        if prio > MQ_PRIO_MAX {
            return Err(Errno::EINVAL);
        }
        if payload.len() > self.queue.msgsize {
            return Err(Errno::EMSGSIZE);
        }

        let nonblocking = self.nonblocking();
        let slot = Mutex::new(Some(payload));
        mq_block(&self.queue.send_wait, nonblocking, timeout_ns, || {
            try_send(&self.queue, &mut slot.lock(), prio)
        })
    }

    /// `mq_receive(3p)` / `mq_timedreceive(3p)`; `timeout_ns` is relative.
    pub fn recv(&self, buf: &mut [u8], timeout_ns: Option<u64>) -> Result<(usize, u32)> {
        // POSIX requires the callee-sized buffer to fit `mq_msgsize`.
        if buf.len() < self.queue.msgsize {
            return Err(Errno::EINVAL);
        }

        let nonblocking = self.nonblocking();
        mq_block(&self.queue.recv_wait, nonblocking, timeout_ns, || {
            try_recv(&self.queue, buf)
        })
    }

    /// Epoll readiness: `IN` when a message awaits, `OUT` when one fits.
    pub fn poll_events(&self) -> PollEvents {
        let mut events = PollEvents::empty();
        if !self.queue.msgs.lock().is_empty() {
            events |= PollEvents::IN | PollEvents::RDNORM;
        }
        if *self.queue.bytes.lock() < self.queue.capacity() {
            events |= PollEvents::OUT | PollEvents::WRNORM;
        }
        events
    }
}

/// The shared wait loop for senders and receivers.
///
/// `step` performs one attempt: `None` means "not ready", staying blocked;
/// `Some(result)` resolves with that result. Non-blocking descriptors do not
/// wait at all; a relative timeout turns into `ETIMEDOUT`. A deadline timer
/// and the queue's own wakeups both race to run the condition re-check, and
/// the condition's state test keeps that race benign.
fn mq_block<F, R>(
    wait_queue: &WaitQueue,
    nonblocking: bool,
    timeout_ns: Option<u64>,
    mut step: F,
) -> Result<R>
where
    F: FnMut() -> Option<Result<R>>,
{
    if let Some(result) = step() {
        return result;
    }
    if nonblocking {
        return Err(Errno::EAGAIN);
    }

    let deadline_ns = timeout_ns.map(|t| crate::time::get_monotonic_ns() + t);
    let timer = timeout_ns.map(|t| {
        // The timer only unblocks; the condition then re-evaluates and
        // reports `ETIMEDOUT` once the deadline has passed.
        crate::kpi::time::arm(
            Duration::from_nanos(t),
            Some(Box::new(move || wait_queue.wake_all())),
        )
    });

    let result = wait_queue.wait_until(|| {
        if let Some(result) = step() {
            return Some(result);
        }
        if Thread::should_interrupt() {
            return Some(Err(Errno::EINTR));
        }
        if deadline_ns.is_some_and(|d| crate::time::get_monotonic_ns() >= d) {
            return Some(Err(Errno::ETIMEDOUT));
        }
        None
    });

    if let Some(timer) = timer {
        timer.cancel();
    }
    result
}

/// The node-ops stub, so a descriptor's inode answers metadata requests the
/// way any anonymous object file does (`fstat(2)`).
struct MqDescNodeOps;

impl NodeOps for MqDescNodeOps {
    fn open(&self, _flags: OpenFlags) -> Result<Box<dyn FileOps>> {
        crate::return_errno!(EOPNOTSUPP, "cannot directly open a message-queue descriptor")
    }

    fn getattr(&self) -> Result<INodeAttr> {
        Ok(INodeAttr {
            size: 0,
            mode: Mode::FIFO | Mode::RUSR | Mode::WUSR,
            nlink: 1,
            uid: 0,
            gid: 0,
            rdev: 0,
        })
    }
}

/// File-operations type behind an `mqd_t`.
pub struct MqDescFileOps {
    desc: Arc<MqDesc>,
}

impl FileOps for MqDescFileOps {
    fn read(&self, _offset: u64, _buf: &mut [u8]) -> Result<usize> {
        // Linux rejects plain read(2) on a `mqd_t`: clients use `mq_receive`.
        // Not ENOSYS — the object exists but has no read semantics.
        Err(Errno::EINVAL)
    }

    fn write(&self, _offset: u64, _buf: &[u8]) -> Result<usize> {
        Err(Errno::EINVAL)
    }

    fn seek(&self, _curr_offset: u64, _anchor: SeekAnchor) -> Result<u64> {
        Err(Errno::ESPIPE)
    }

    fn poll(&self) -> PollEvents {
        self.desc.poll_events()
    }

    fn as_mqdesc(&self) -> Option<Arc<MqDesc>> {
        Some(self.desc.clone())
    }
}

/// Builds the descriptor [`File`] handed out by `mq_open(2)`.
const MQ_DESC_INODE_NUMBER: usize = 904;

pub fn create_mqdesc_file(
    queue: Arc<Mqueue>,
    nonblocking: bool,
) -> Arc<File> {
    let desc = Arc::new(MqDesc::new(queue, nonblocking));

    let inode = Arc::new(INode::new(
        Box::new(MqDescNodeOps),
        Mode::FIFO | Mode::RUSR | Mode::WUSR,
        MQ_DESC_INODE_NUMBER,
    ));

    let mut open_flags = OpenFlags::READ | OpenFlags::WRITE;
    if nonblocking {
        open_flags |= OpenFlags::NONBLOCK;
    }

    Arc::new(File::new(
        Box::new(MqDescFileOps { desc }),
        inode,
        open_flags,
        false,
    ))
}
