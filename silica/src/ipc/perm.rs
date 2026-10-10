// SPDX-License-Identifier: GPL-2.0

//! System V IPC permission checks and validation.

use crate::{
    api::{
        cred::Ucred,
        errno::{Errno, Result},
        ipc::IpcPerm,
    },
};

/// Required mode bits for SysV IPC operations.
pub const IPC_R: u32 = 0o4;
pub const IPC_W: u32 = 0o2;

/// Validates that caller `cred` has read access to the IPC object described by `perm`.
pub fn check_read_permission(perm: &IpcPerm, cred: &Ucred) -> Result<()> {
    if perm.check_perm(cred, IPC_R) {
        Ok(())
    } else {
        Err(Errno::EACCES)
    }
}

/// Validates that caller `cred` has write/alter access to the IPC object described by `perm`.
pub fn check_write_permission(perm: &IpcPerm, cred: &Ucred) -> Result<()> {
    if perm.check_perm(cred, IPC_W) {
        Ok(())
    } else {
        Err(Errno::EACCES)
    }
}

/// Validates that caller `cred` has administrative control access (owner, creator, or root)
/// for commands like `IPC_RMID` or `IPC_SET`.
pub fn check_admin_permission(perm: &IpcPerm, cred: &Ucred) -> Result<()> {
    if cred.cr_uid.is_root()
        || cred.cr_uid.as_u32() == perm.uid
        || cred.cr_uid.as_u32() == perm.cuid
    {
        Ok(())
    } else {
        Err(Errno::EPERM)
    }
}
