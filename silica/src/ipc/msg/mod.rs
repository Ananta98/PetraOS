// SPDX-License-Identifier: GPL-2.0

//! System V Message Queue subsystem.
//!
//! Implements message queuing, priority/type-based reception, atomic enqueue/dequeue,
//! and full IPC permission checks in 100% safe Rust.

pub mod message;
pub mod queue;

use alloc::sync::Arc;
use alloc::vec::Vec;
use ostd::sync::{SpinLock, Waiter};

use self::message::Message;
use self::queue::MsgQueue;
use crate::ipc::perm::{check_admin_permission, check_read_permission, check_write_permission};
use crate::ipc::registry::IpcRegistry;
use crate::proc::thread::{Thread, ThreadFlags, ThreadState};
use crate::proc::{Proc, ProcState};
use crate::uapi::cred::Ucred;
use crate::uapi::errno::{Errno, Result};
use crate::uapi::ipc::*;
use crate::uapi::msg::*;
use crate::utils::userspace::user_ptr::UserPtr;

/// Global registry of message queues by `msqid`.
static MSG_REGISTRY: IpcRegistry<SpinLock<MsgQueue>> = IpcRegistry::new();

/// Returns current system timestamp in seconds using the active clocksource.
fn current_time_secs() -> i64 {
    crate::time::realtime_secs()
}

/// Cancels a blocking IPC wait once the caller has been interrupted or is
/// dying: an AST is outstanding, termination is pending, the thread is dead,
/// or the whole process has become a zombie.
fn wait_cancelled() -> Result<()> {
    if let Some(td) = Thread::current() {
        let inner = td.inner.lock();
        if inner.flags.contains(ThreadFlags::TDF_ASTPENDING)
            || inner.flags.contains(ThreadFlags::KILL_PENDING)
            || inner.state == ThreadState::Dead
        {
            return Err(Errno::EINTR);
        }
    }
    if let Some(proc) = Thread::current_proc() {
        if proc.state() == ProcState::Zombie {
            return Err(Errno::EINTR);
        }
    }
    Ok(())
}

/// Looks up a message queue by `msqid`.
pub fn lookup_queue(msqid: i32) -> Option<Arc<SpinLock<MsgQueue>>> {
    MSG_REGISTRY.get(msqid)
}

/// Handles `msgget(2)`.
pub fn msg_get(key: Key, msgflg: i32, cred: &Ucred) -> Result<i32> {
    let existing_msqid = MSG_REGISTRY.lookup_key(key);

    if let Some(msqid) = existing_msqid {
        if (msgflg & IPC_CREAT != 0) && (msgflg & IPC_EXCL != 0) {
            return Err(Errno::EEXIST);
        }

        let q_lock = lookup_queue(msqid).ok_or(Errno::ENOENT)?;
        let q = q_lock.lock();
        if q.is_deleted {
            return Err(Errno::EIDRM);
        }

        check_read_permission(&q.ds.msg_perm, cred)?;
        return Ok(msqid);
    }

    if msgflg & IPC_CREAT == 0 {
        return Err(Errno::ENOENT);
    }

    create_msg_queue(key, msgflg, cred)
}

/// Creates a new message queue.
fn create_msg_queue(key: Key, msgflg: i32, cred: &Ucred) -> Result<i32> {
    let mut reg = MSG_REGISTRY.lock();
    if reg.len() >= MSGMNI {
        return Err(Errno::ENOSPC);
    }

    let msqid = crate::ipc::id::alloc_ipc_id(&reg);
    let now = current_time_secs();

    let ds = MsqIdDs {
        msg_perm: IpcPerm::new(key, cred, msgflg as u32),
        msg_stime: 0,
        msg_rtime: 0,
        msg_ctime: now,
        msg_cbytes: 0,
        msg_qnum: 0,
        msg_qbytes: MSGMNB as u64,
        msg_lspid: 0,
        msg_lrpid: 0,
        __glibc_reserved4: 0,
        __glibc_reserved5: 0,
    };

    let q = Arc::new(SpinLock::new(MsgQueue::new(ds)));
    reg.insert(msqid, Arc::clone(&q));
    MSG_REGISTRY.bind_key(key, msqid);

    Ok(msqid)
}

/// Handles `msgsnd(2)`.
pub fn msg_snd(msqid: i32, mtype: i64, mtext: &[u8], msgflg: i32, proc: &Arc<Proc>) -> Result<()> {
    if mtype <= 0 {
        return Err(Errno::EINVAL);
    }
    if mtext.len() > MSGMAX {
        return Err(Errno::EINVAL);
    }

    let q_lock = lookup_queue(msqid).ok_or(Errno::EINVAL)?;
    let cred = proc.cred();

    // Check write permissions
    {
        let q = q_lock.lock();
        if q.is_deleted {
            return Err(Errno::EIDRM);
        }
        check_write_permission(&q.ds.msg_perm, &cred)?;
    }

    let msgsz = mtext.len();

    // Fast path: can we enqueue immediately?
    {
        let mut q = q_lock.lock();
        if q.is_deleted {
            return Err(Errno::EIDRM);
        }
        if q.ds.msg_cbytes + msgsz as u64 <= q.ds.msg_qbytes && q.ds.msg_qnum < MSGMNB as u64 {
            let msg = Message::new(mtype, mtext.to_vec());
            q.messages.push_back(msg);
            q.ds.msg_cbytes += msgsz as u64;
            q.ds.msg_qnum += 1;
            q.ds.msg_lspid = proc.pid.as_u32();
            q.ds.msg_stime = current_time_secs();
            q.recv_wq.wake_all();
            return Ok(());
        }
    }

    if msgflg & IPC_NOWAIT != 0 {
        return Err(Errno::EAGAIN);
    }

    // Slow path: block on wait queue
    let (waiter, _) = Waiter::new_pair();

    let cond = || {
        let mut q = q_lock.lock();
        if q.is_deleted {
            return Some(Err(Errno::EIDRM));
        }

        if q.ds.msg_cbytes + msgsz as u64 <= q.ds.msg_qbytes && q.ds.msg_qnum < MSGMNB as u64 {
            let msg = Message::new(mtype, mtext.to_vec());
            q.messages.push_back(msg);
            q.ds.msg_cbytes += msgsz as u64;
            q.ds.msg_qnum += 1;
            q.ds.msg_lspid = proc.pid.as_u32();
            q.ds.msg_stime = current_time_secs();
            q.recv_wq.wake_all();
            Some(Ok(()))
        } else {
            q.send_wq.enqueue(waiter.waker());
            None
        }
    };

    let cancel_cond = wait_cancelled;

    let wait_res = waiter.wait_until_or_cancelled(cond, cancel_cond);
    match wait_res {
        Ok(inner_res) => inner_res,
        Err(e) => Err(e),
    }
}

/// Helper to find matching message index according to `msgtyp` and `msgflg`.
fn find_message_index(q: &MsgQueue, msgtyp: i64, msgflg: i32) -> Option<usize> {
    if (msgflg & MSG_COPY) != 0 {
        if msgtyp < 0 {
            return None;
        }
        let idx = msgtyp as usize;
        if idx < q.messages.len() {
            return Some(idx);
        }
        return None;
    }

    if msgtyp == 0 {
        if !q.messages.is_empty() {
            return Some(0);
        }
        return None;
    }

    if msgtyp > 0 {
        let except = (msgflg & MSG_EXCEPT) != 0;
        for (i, m) in q.messages.iter().enumerate() {
            let matches = if except {
                m.mtype != msgtyp
            } else {
                m.mtype == msgtyp
            };
            if matches {
                return Some(i);
            }
        }
        return None;
    }

    // msgtyp < 0: return first message with lowest type that is <= -msgtyp
    let max_typ = -msgtyp;
    let mut min_typ = i64::MAX;
    let mut best_idx = None;

    for (i, m) in q.messages.iter().enumerate() {
        if m.mtype <= max_typ && m.mtype < min_typ {
            min_typ = m.mtype;
            best_idx = Some(i);
        }
    }

    best_idx
}

/// Handles `msgrcv(2)`.
pub fn msg_rcv(
    msqid: i32,
    msgsz: usize,
    msgtyp: i64,
    msgflg: i32,
    proc: &Arc<Proc>,
) -> Result<(i64, Vec<u8>)> {
    if (msgflg & MSG_COPY) != 0 && ((msgflg & IPC_NOWAIT) == 0 || msgtyp < 0) {
        return Err(Errno::EINVAL);
    }

    let q_lock = lookup_queue(msqid).ok_or(Errno::EINVAL)?;
    let cred = proc.cred();

    // Check read permissions
    {
        let q = q_lock.lock();
        if q.is_deleted {
            return Err(Errno::EIDRM);
        }
        check_read_permission(&q.ds.msg_perm, &cred)?;
    }

    // Fast path: try to receive message immediately
    {
        let mut q = q_lock.lock();
        if q.is_deleted {
            return Err(Errno::EIDRM);
        }

        if let Some(idx) = find_message_index(&q, msgtyp, msgflg) {
            let msg_len = q.messages[idx].size();
            if msg_len > msgsz && (msgflg & MSG_NOERROR == 0) {
                return Err(Errno::E2BIG);
            }

            let msg = if (msgflg & MSG_COPY) != 0 {
                q.messages[idx].clone()
            } else {
                let m = q.messages.remove(idx).unwrap();
                q.ds.msg_cbytes -= m.size() as u64;
                q.ds.msg_qnum -= 1;
                q.ds.msg_lrpid = proc.pid.as_u32();
                q.ds.msg_rtime = current_time_secs();
                q.send_wq.wake_all();
                m
            };

            let take_len = msg.mtext.len().min(msgsz);
            let payload = msg.mtext[..take_len].to_vec();
            return Ok((msg.mtype, payload));
        }
    }

    if msgflg & IPC_NOWAIT != 0 {
        return Err(Errno::ENOMSG);
    }

    // Slow path: block on wait queue
    let (waiter, _) = Waiter::new_pair();

    let cond = || {
        let mut q = q_lock.lock();
        if q.is_deleted {
            return Some(Err(Errno::EIDRM));
        }

        if let Some(idx) = find_message_index(&q, msgtyp, msgflg) {
            let msg_len = q.messages[idx].size();
            if msg_len > msgsz && (msgflg & MSG_NOERROR == 0) {
                return Some(Err(Errno::E2BIG));
            }

            let msg = if (msgflg & MSG_COPY) != 0 {
                q.messages[idx].clone()
            } else {
                let m = q.messages.remove(idx).unwrap();
                q.ds.msg_cbytes -= m.size() as u64;
                q.ds.msg_qnum -= 1;
                q.ds.msg_lrpid = proc.pid.as_u32();
                q.ds.msg_rtime = current_time_secs();
                q.send_wq.wake_all();
                m
            };

            let take_len = msg.mtext.len().min(msgsz);
            let payload = msg.mtext[..take_len].to_vec();
            Some(Ok((msg.mtype, payload)))
        } else {
            q.recv_wq.enqueue(waiter.waker());
            None
        }
    };

    let cancel_cond = wait_cancelled;

    let wait_res = waiter.wait_until_or_cancelled(cond, cancel_cond);
    match wait_res {
        Ok(inner_res) => inner_res,
        Err(e) => Err(e),
    }
}

/// Handles `msgctl(2)`.
pub fn msg_ctl(
    msqid: i32,
    cmd: i32,
    buf_ptr: usize,
    cred: &Ucred,
    proc: &Arc<Proc>,
) -> Result<usize> {
    let vmar = proc.vmspace();

    if cmd == IPC_INFO || cmd == MSG_INFO {
        if buf_ptr == 0 {
            return Err(Errno::EFAULT);
        }
        let info = MsgInfo::default();
        UserPtr::new(buf_ptr).write_bytes(&vmar, &info.to_bytes())?;
        return Ok(MSGMNI);
    }

    if cmd == MSG_STAT {
        if buf_ptr == 0 {
            return Err(Errno::EFAULT);
        }
        let reg = MSG_REGISTRY.lock();
        let idx = msqid as usize;
        let (&id, q_lock) = reg.iter().nth(idx).ok_or(Errno::EINVAL)?;
        let q = q_lock.lock();
        check_read_permission(&q.ds.msg_perm, cred)?;
        let bytes = q.ds.to_bytes();
        drop(q);
        drop(reg);

        UserPtr::new(buf_ptr).write_bytes(&vmar, &bytes)?;
        return Ok(id as usize);
    }

    let q_lock = lookup_queue(msqid).ok_or(Errno::EINVAL)?;

    match cmd {
        IPC_RMID => {
            let mut q = q_lock.lock();
            if q.is_deleted {
                return Err(Errno::EIDRM);
            }
            check_admin_permission(&q.ds.msg_perm, cred)?;

            q.is_deleted = true;
            let key = q.ds.msg_perm.key;

            let mut reg = MSG_REGISTRY.lock();
            reg.remove(&msqid);
            MSG_REGISTRY.unbind_key(key);

            // Wake all waiting tasks with EIDRM
            q.send_wq.wake_all();
            q.recv_wq.wake_all();

            Ok(0)
        }
        IPC_SET => {
            if buf_ptr == 0 {
                return Err(Errno::EFAULT);
            }
            let mut raw = [0u8; MsqIdDs::SIZE];
            UserPtr::new(buf_ptr).read_bytes(&vmar, &mut raw)?;
            let new_ds = MsqIdDs::from_bytes(&raw).ok_or(Errno::EINVAL)?;

            let mut q = q_lock.lock();
            if q.is_deleted {
                return Err(Errno::EIDRM);
            }
            check_admin_permission(&q.ds.msg_perm, cred)?;

            if new_ds.msg_qbytes > q.ds.msg_qbytes && !cred.is_superuser() {
                return Err(Errno::EPERM);
            }

            q.ds.msg_perm.uid = new_ds.msg_perm.uid;
            q.ds.msg_perm.gid = new_ds.msg_perm.gid;
            q.ds.msg_perm.mode = (q.ds.msg_perm.mode & !0o777) | (new_ds.msg_perm.mode & 0o777);
            q.ds.msg_qbytes = new_ds.msg_qbytes;
            q.ds.msg_ctime = current_time_secs();

            Ok(0)
        }
        IPC_STAT => {
            if buf_ptr == 0 {
                return Err(Errno::EFAULT);
            }
            let q = q_lock.lock();
            if q.is_deleted {
                return Err(Errno::EIDRM);
            }
            check_read_permission(&q.ds.msg_perm, cred)?;
            let bytes = q.ds.to_bytes();
            drop(q);

            UserPtr::new(buf_ptr).write_bytes(&vmar, &bytes)?;
            Ok(0)
        }
        _ => Err(Errno::EINVAL),
    }
}

/// Cleanup on process exit.
///
/// Wakes any tasks waiting on message queues (send or receive wait queues),
/// allowing threads associated with the exiting process to observe cancellation
/// or termination state (`TDF_ASTPENDING`, `KILL_PENDING`, `ThreadState::Dead`,
/// or `ProcState::Zombie`) and return cleanly without deadlocking.
pub fn on_proc_exit(_proc: &Proc) {
    let queues: Vec<Arc<SpinLock<MsgQueue>>> = {
        let reg = MSG_REGISTRY.lock();
        reg.values().cloned().collect()
    };

    for q_lock in queues {
        let q = q_lock.lock();
        if !q.is_deleted {
            q.send_wq.wake_all();
            q.recv_wq.wake_all();
        }
    }
}
