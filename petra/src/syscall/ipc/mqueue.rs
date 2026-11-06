// SPDX-License-Identifier: GPL-2.0

//! POSIX message-queue system calls (`mq_open(2)` through `mq_getsetattr(2)`).
//!
//! On x86_64 these occupy syscalls 240-245.

use alloc::string::String;
use ostd::arch::cpu::context::UserContext;

use crate::{
    fs::fd::FdFlags,
    ipc::mqueue::{
        create_mqdesc_file, mq_register_notify, queue_open, queue_unlink, MqAttrSnapshot, MqDesc,
        O_CLOEXEC, O_EXCL, O_NONBLOCK, O_CREAT,
    },
    proc::thread::Thread,
    uapi::errno::{Errno, Result},
    utils::UserPtr,
};

/// `O_RDONLY`-family accessor bits from open(2).
const O_ACCMODE: i32 = 0o3;
/// `(sigev_value, sigev_signo, sigev_notify, ...)` in one 32-byte read.
const SIGEV_RAW_SIZE: usize = 32;
/// Whole `struct mq_attr` size on x86_64 (four `long`s plus padding).
const MQ_ATTR_RAW_SIZE: usize = 48;

/// Decodes the accessor bits into `(may-read?, may-write?)`.
fn access_from_flags(oflag: i32) -> (bool, bool) {
    match oflag & O_ACCMODE {
        0 => (true, false),
        1 => (false, true),
        _ => (true, true),
    }
}

/// Valid `mq_open(2)`-family flag mask.
fn mq_open_flags_valid(oflag: i32, for_open: bool) -> bool {
    let known = O_ACCMODE | O_CREAT | O_EXCL | O_NONBLOCK | O_CLOEXEC;
    let create_only = O_CREAT | O_EXCL;
    (oflag & !known) == 0 && (!for_open || (oflag & create_only) == 0)
}

/// Validates a relative `struct timespec` timeout pointer.
fn read_relative_timeout(timeout_ptr: usize, vmar: &crate::vm::vmar::Vmar) -> Result<Option<u64>> {
    if timeout_ptr == 0 {
        return Ok(None);
    }
    let mut raw = [0u8; 16];
    UserPtr::new(timeout_ptr).read_bytes(vmar, &mut raw)?;
    let tv_sec = i64::from_ne_bytes(raw[..8].try_into().unwrap_or_default());
    let tv_nsec = i64::from_ne_bytes(raw[8..].try_into().unwrap_or_default());
    if tv_sec < 0 || tv_nsec < 0 || tv_nsec >= 1_000_000_000 {
        return Err(Errno::EINVAL);
    }
    let sec_ns = tv_sec.checked_mul(1_000_000_000).ok_or(Errno::EOVERFLOW)?;
    let total = u64::try_from(sec_ns).ok_or(Errno::EOVERFLOW)? + tv_nsec as u64;
    Ok(Some(total))
}

/// Fetches the descriptor behind an open `mqd_t`.
fn mqdesc_from_fd(mqdes: i32) -> Result<alloc::sync::Arc<MqDesc>> {
    let proc = Thread::proc_or_esrch()?;
    let file = proc.fd_table.lock().get(mqdes)?;
    file.as_mqdesc().ok_or(Errno::EBADF)
}/// System call `mq_open(const char *name, int oflag, mode_t mode, struct mq_attr *attr)`
/// (Syscall #240).
pub fn sys_mq_open(
    name_ptr: usize,
    oflag: i32,
    mode: u32,
    attr_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();
    let name: String = UserPtr::new(name_ptr).read_string(&vmar, 4096)?;

    mq_open_flags_valid(oflag, false).then_some(()).ok_or_else(|| Errno::EINVAL)?;

    let use_attrs = (oflag & O_CREAT) != 0 && attr_ptr != 0;
    let (attr_maxmsg, attr_msgsize) = if use_attrs {
        let mut raw = [0u8; MQ_ATTR_RAW_SIZE];
        UserPtr::new(attr_ptr).read_bytes(&vmar, &mut raw)?;
        (
            u64::from_ne_bytes(raw[8..16].try_into().unwrap_or_default()) as usize,
            u64::from_ne_bytes(raw[16..24].try_into().unwrap_or_default()) as usize,
        )
    } else {
        (0, 0)
    };

    let (queue, _) = queue_open(&name, oflag, mode, use_attrs, attr_maxmsg, attr_msgsize)?;

    let (may_read, _may_write) = access_from_flags(oflag);
    let nonblocking = (oflag & O_NONBLOCK) != 0;
    let _ = may_read;    let file = create_mqdesc_file(queue, nonblocking);

    let mut fd_flags = FdFlags::empty();
    if (oflag & O_CLOEXEC) != 0 {
        fd_flags |= FdFlags::FD_CLOEXEC;
    }
    let mq_fd = proc.fd_table.lock().alloc_fd(file, fd_flags)?;
    Ok(mq_fd as usize)
}

/// System call `mq_unlink(const char *name)` (Syscall #241).
pub fn sys_mq_unlink(name_ptr: usize, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();
    let name = UserPtr::new(name_ptr).read_string(&vmar, 4096)?;
    queue_unlink(&name)?;
    Ok(0)
}

/// System call `mq_timedsend(mqd_t mqdes, const char *msg_ptr, size_t msg_len,
/// unsigned int msg_prio, const struct timespec *abs_timeout)` (Syscall #242).
///
/// The `x86_64` variant takes a **relative** timeout.
pub fn sys_mq_timedsend(
    mqdes: i32,
    msg_ptr: usize,
    msg_len: usize,
    msg_prio: u32,
    timeout_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if mqdes < 0 {
        return Err(Errno::EBADF);
    }
    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();
    let desc = mqdesc_from_fd(mqdes)?;

    if msg_ptr == 0 {
        return Err(Errno::EFAULT);
    }
    let mut payload = alloc::vec![0u8; msg_len];
    if msg_len > 0 {
        UserPtr::new(msg_ptr).read_bytes(&vmar, &mut payload)?;
    }

    let timeout_ns = read_relative_timeout(timeout_ptr, &vmar)?;
    desc.send(payload, msg_prio, timeout_ns)
}

/// System call `mq_timedreceive(mqd_t mqdes, char *msg_ptr, size_t msg_len,
/// unsigned int *msg_prio, const struct timespec *abs_timeout)` (Syscall #243).
///
/// The `x86_64` variant takes a **relative** timeout.
pub fn sys_mq_timedreceive(
    mqdes: i32,
    msg_ptr: usize,
    msg_len: usize,
    msg_prio_ptr: usize,
    timeout_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if mqdes < 0 || msg_ptr == 0 {
        return Err(Errno::EFAULT);
    }
    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();
    let desc = mqdesc_from_fd(mqdes)?;

    let timeout_ns = read_relative_timeout(timeout_ptr, &vmar)?;
    let mut payload = alloc::vec![0u8; msg_len];
    let (received, prio) = desc.recv(&mut payload, timeout_ns)?;

    UserPtr::new(msg_ptr).write_bytes(&vmar, &payload[..received])?;
    if msg_prio_ptr != 0 {
        UserPtr::new(msg_prio_ptr).write_bytes(&vmar, &prio.to_ne_bytes())?;
    }
    Ok(received)
}

/// System call `mq_notify(mqd_t mqdes, const struct sigevent *sevp)` (Syscall #244).
pub fn sys_mq_notify(sevp_ptr: usize, mqdes: i32, _ctx: &mut UserContext) -> Result<usize> {
    if mqdes < 0 {
        return Err(Errno::EBADF);
    }
    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();
    let desc = mqdesc_from_fd(mqdes)?;

    if sevp_ptr == 0 {
        mq_register_notify(&desc.queue, 0, 0)?;
        return Ok(0);
    }

    let mut raw = [0u8; SIGEV_RAW_SIZE];
    UserPtr::new(sevp_ptr).read_bytes(&vmar, &mut raw)?;
    // x86_64 struct sigevent: sigev_value @0 (8 B), sigev_signo @8,
    // sigev_notify @12.
    let signo = i32::from_ne_bytes(raw[8..12].try_into().unwrap_or_default());
    let notify = u32::from_ne_bytes(raw[12..16].try_into().unwrap_or_default());

    match notify {
        // SIGEV_NONE
        0 => mq_register_notify(&desc.queue, 0, 0)?,
        // SIGEV_SIGNAL
        1 => {
            if !(1..=64).contains(&signo) {
                return Err(Errno::EINVAL);
            }
            let value = u64::from_ne_bytes(raw[..8].try_into().unwrap_or_default());
            mq_register_notify(&desc.queue, signo, value)?;
        }
        // SIGEV_THREAD / SIGEV_THREAD_ID: no thread-notify plumbing yet.
        _ => return Err(Errno::ENOTSUP),
    }
    Ok(0)
}

/// System call `mq_getsetattr(mqd_t mqdes, const struct mq_attr *newattr,
/// struct mq_attr *oldattr)` (Syscall #245).
pub fn sys_mq_getsetattr(
    mqdes: i32,
    newattr_ptr: usize,
    oldattr_ptr: usize,
    _ctx: &mut UserContext,
) -> Result<usize> {
    if mqdes < 0 {
        return Err(Errno::EBADF);
    }
    let proc = Thread::proc_or_esrch()?;
    let vmar = proc.vmspace();
    let desc = mqdesc_from_fd(mqdes)?;

    if oldattr_ptr != 0 {
        let snapshot = desc.attr_snapshot();
        UserPtr::new(oldattr_ptr).write_val(&vmar, snapshot)?;
    }

    if newattr_ptr != 0 {
        let mut raw = [0u8; MQ_ATTR_RAW_SIZE];
        UserPtr::new(newattr_ptr).read_bytes(&vmar, &mut raw)?;
        let mq_flags = u64::from_ne_bytes(raw[..8].try_into().unwrap_or_default());
        // Only O_NONBLOCK is settable on an open descriptor.
        if (mq_flags & !(O_NONBLOCK as u64)) != 0 {
            return Err(Errno::EINVAL);
        }
        desc.set_nonblocking((mq_flags & O_NONBLOCK as u64) != 0);
    }
    Ok(0)
}
