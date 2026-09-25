//! System calls for changing file ownership (`chown`, `fchown`, `lchown`, `fchownat`).

use super::*;
use crate::fs::vfs::types::Stat;
use crate::syscalls::{wrap_syscall, SyscallError, SyscallResult, UserCStr};
use alloc::sync::Arc;

/// Substitute the `(uid_t)-1` / `(gid_t)-1` "leave unchanged" sentinels with
/// the ownership currently recorded in `st`.
pub(crate) fn effective_owner(st: &Stat, uid: u32, gid: u32) -> (u32, u32) {
    (
        if uid == u32::MAX { st.uid } else { uid },
        if gid == u32::MAX { st.gid } else { gid },
    )
}

/// `sys_chown` (SYS_CHOWN = 92)
/// Change ownership of a file.
#[wrap_syscall]
pub fn sys_chown(path_ptr: UserCStr, uid: u32, gid: u32) -> SyscallResult {
    let path = path_ptr.to_string(256)?;
    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    let st = crate::fs::stat(&full_path)?;
    let (uid, gid) = effective_owner(&st, uid, gid);

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let creds = { Arc::clone(&proc_arc.lock().creds) };

    if creds.euid != 0 {
        if uid != st.uid || (gid != st.gid && gid != creds.gid && gid != creds.egid) {
            return Err(SyscallError::EPERM);
        }
    }

    crate::fs::chown(&full_path, uid, gid)?;
    Ok(0)
}

/// `sys_fchown` (SYS_FCHOWN = 93)
/// Change ownership of an open file descriptor.
#[wrap_syscall]
pub fn sys_fchown(fd: i32, uid: u32, gid: u32) -> SyscallResult {
    if fd < 0 {
        return Err(SyscallError::EBADF);
    }
    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let proc = proc_arc.lock();
    let file = proc.fd_table.get(fd)?;
    let creds = Arc::clone(&proc.creds);
    drop(proc);

    let st = file.dentry.inode.ops.stat()?;
    let (uid, gid) = effective_owner(&st, uid, gid);

    if creds.euid != 0 {
        if uid != st.uid || (gid != st.gid && gid != creds.gid && gid != creds.egid) {
            return Err(SyscallError::EPERM);
        }
    }

    file.ops
        .chown(uid, gid)
        .or_else(|_| file.dentry.inode.ops.chown(uid, gid))?;
    Ok(0)
}

/// `sys_lchown` (SYS_LCHOWN = 94)
/// Change ownership of a file without following symlinks (root only,
/// except owner-preserving group changes mirroring `chown`).
#[wrap_syscall]
pub fn sys_lchown(path_ptr: UserCStr, uid: u32, gid: u32) -> SyscallResult {
    let path = path_ptr.to_string(256)?;
    let full_path = resolve_at_path(AT_FDCWD, &path)?;
    let dentry = crate::fs::resolve_path_nofollow(&full_path)?;
    let st = dentry.inode.ops.stat()?;
    let (uid, gid) = effective_owner(&st, uid, gid);

    let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
    let creds = { Arc::clone(&proc_arc.lock().creds) };
    if creds.euid != 0 {
        if uid != st.uid || (gid != st.gid && gid != creds.gid && gid != creds.egid) {
            return Err(SyscallError::EPERM);
        }
    }

    dentry.inode.ops.chown(uid, gid)?;
    Ok(0)
}

/// `sys_fchownat` (SYS_FCHOWNAT = 260)
/// Change ownership of a file relative to a directory file descriptor.
#[wrap_syscall]
pub fn sys_fchownat(dfd: i32, path_ptr: UserCStr, uid: u32, gid: u32, flags: i32) -> SyscallResult {
    let path = path_ptr.to_string(256)?;
    let full_path = resolve_at_path(dfd, &path)?;
    if (flags & crate::fs::AT_SYMLINK_NOFOLLOW) != 0 {
        let dentry = crate::fs::resolve_path_nofollow(&full_path)?;
        let st = dentry.inode.ops.stat()?;
        let (uid, gid) = effective_owner(&st, uid, gid);
        let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
        let creds = { Arc::clone(&proc_arc.lock().creds) };
        if creds.euid != 0 {
            if uid != st.uid || (gid != st.gid && gid != creds.gid && gid != creds.egid) {
                return Err(SyscallError::EPERM);
            }
        }
        dentry.inode.ops.chown(uid, gid)?;
    } else {
        let st = crate::fs::stat(&full_path)?;
        let (uid, gid) = effective_owner(&st, uid, gid);
        let proc_arc = crate::proc::current_process().ok_or(SyscallError::ESRCH)?;
        let creds = { Arc::clone(&proc_arc.lock().creds) };
        if creds.euid != 0 {
            if uid != st.uid || (gid != st.gid && gid != creds.gid && gid != creds.egid) {
                return Err(SyscallError::EPERM);
            }
        }
        crate::fs::chown(&full_path, uid, gid)?;
    }
    Ok(0)
}
