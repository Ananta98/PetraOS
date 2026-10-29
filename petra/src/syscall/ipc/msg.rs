// SPDX-License-Identifier: GPL-2.0

//! System V Message Queue system calls.

use ostd::arch::cpu::context::UserContext;

use crate::{
    proc::Thread,
    uapi::errno::{Errno, Result},
    utils::UserPtr,
};

/// `msgget(2)` - Allocates or accesses a System V message queue.
pub fn sys_msgget(key: i32, msgflg: i32, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let cred = proc.cred();
    let msqid = crate::ipc::msg::msg_get(key, msgflg, &cred)?;
    Ok(msqid as usize)
}

/// `msgsnd(2)` - Sends a message to a System V message queue.
pub fn sys_msgsnd(
    msqid: i32,
    msgp: usize,
    msgsz: usize,
    msgflg: i32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    if msgp == 0 {
        return Err(Errno::EFAULT);
    }
    if msgsz > crate::uapi::msg::MSGMAX {
        return Err(Errno::EINVAL);
    }

    let vmar = proc.vmspace();
    let mut raw_type = [0u8; 8];
    UserPtr::new(msgp).read_bytes(&vmar, &mut raw_type)?;
    let mtype = i64::from_ne_bytes(raw_type);
    if mtype <= 0 {
        return Err(Errno::EINVAL);
    }

    let mut payload = alloc::vec![0u8; msgsz];
    if msgsz > 0 {
        let text_ptr = msgp.checked_add(8).ok_or(Errno::EFAULT)?;
        UserPtr::new(text_ptr).read_bytes(&vmar, &mut payload)?;
    }

    crate::ipc::msg::msg_snd(msqid, mtype, &payload, msgflg, &proc)?;
    Ok(0)
}

/// `msgrcv(2)` - Receives a message from a System V message queue.
pub fn sys_msgrcv(
    msqid: i32,
    msgp: usize,
    msgsz: usize,
    msgtyp: i64,
    msgflg: i32,
    _ctx: &mut UserContext,
) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    if msgp == 0 {
        return Err(Errno::EFAULT);
    }

    let (mtype, payload) = crate::ipc::msg::msg_rcv(msqid, msgsz, msgtyp, msgflg, &proc)?;
    let vmar = proc.vmspace();
    UserPtr::new(msgp).write_bytes(&vmar, &mtype.to_ne_bytes())?;
    if !payload.is_empty() {
        let text_ptr = msgp.checked_add(8).ok_or(Errno::EFAULT)?;
        UserPtr::new(text_ptr).write_bytes(&vmar, &payload)?;
    }

    Ok(payload.len())
}

/// `msgctl(2)` - System V message queue control operations.
pub fn sys_msgctl(msqid: i32, cmd: i32, buf: usize, _ctx: &mut UserContext) -> Result<usize> {
    let proc = Thread::proc_or_esrch()?;
    let cred = proc.cred();
    crate::ipc::msg::msg_ctl(msqid, cmd, buf, &cred, &proc)
}
