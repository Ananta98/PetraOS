//! POSIX file-permission checks against process credentials.
//!
//! Single source of truth for uid/gid/mode evaluation so `access`,
//! `open` and `exec` share one implementation (DRY).

use super::types::{Stat, VfsError};
use alloc::vec::Vec;

/// `access(2)` mode bits.
pub const F_OK: u32 = 0;
pub const X_OK: u32 = 1;
pub const W_OK: u32 = 2;
pub const R_OK: u32 = 4;

/// `faccessat(2)` flag requesting effective-id checks.
pub const AT_EACCESS: i32 = 0x200;

/// Snapshot of the calling process identity relevant to permission checks.
#[derive(Debug, Clone)]
pub struct Identity {
    pub uid: u32,
    pub gid: u32,
    pub euid: u32,
    pub egid: u32,
    pub fsuid: u32,
    pub fsgid: u32,
    pub supplementary_gids: Vec<u32>,
    pub umask: u32,
}

impl Identity {
    fn root() -> Self {
        Self {
            uid: 0,
            gid: 0,
            euid: 0,
            egid: 0,
            fsuid: 0,
            fsgid: 0,
            supplementary_gids: Vec::new(),
            umask: 0o022,
        }
    }

    /// IDs used for filesystem checks (fsuid/fsgid per Linux).
    fn fs_pair(&self, effective: bool) -> (u32, u32) {
        if effective {
            (self.fsuid, self.fsgid)
        } else {
            (self.uid, self.gid)
        }
    }

    fn is_member(&self, gid: u32, effective: bool) -> bool {
        if effective {
            self.fsgid == gid || self.supplementary_gids.contains(&gid)
        } else {
            self.gid == gid || self.supplementary_gids.contains(&gid)
        }
    }
}

/// Current process identity, or root during early boot (no process yet).
pub fn current_identity() -> Identity {
    let proc_opt = crate::proc::current_process();
    let proc_arc = match proc_opt {
        Some(p) => p,
        None => return Identity::root(),
    };
    let proc = proc_arc.lock();
    Identity {
        uid: proc.creds.uid,
        gid: proc.creds.gid,
        euid: proc.creds.euid,
        egid: proc.creds.egid,
        fsuid: proc.creds.fsuid,
        fsgid: proc.creds.fsgid,
        supplementary_gids: proc.creds.supplementary_gids.clone(),
        umask: proc.umask,
    }
}

/// Creator ownership for newly created files (fsuid/fsgid).
pub fn creator_owner() -> (u32, u32) {
    let id = current_identity();
    (id.fsuid, id.fsgid)
}

/// Apply the process umask to a creation mode.
pub fn apply_umask(mode: u32, umask: u32) -> u32 {
    mode & !umask
}

/// Core permission evaluation on an already-fetched [`Stat`].
///
/// `use_effective` selects euid/egid (`faccessat(AT_EACCESS)`, `open`,
/// `exec`) vs ruid/rgid (`access`).
pub fn can_access_stat(stat: &Stat, mode: u32, use_effective: bool) -> bool {
    if mode == F_OK {
        return true;
    }
    let id = current_identity();
    let (uid, _) = id.fs_pair(use_effective);

    // Root bypasses permission checks, but needs at least one exec bit set.
    if uid == 0 {
        if (mode & X_OK) != 0 {
            return (stat.mode & 0o111) != 0;
        }
        return true;
    }

    let want_read = (mode & R_OK) != 0;
    let want_write = (mode & W_OK) != 0;
    let want_exec = (mode & X_OK) != 0;

    if stat.uid == uid {
        if want_read && (stat.mode & 0o400) == 0 {
            return false;
        }
        if want_write && (stat.mode & 0o200) == 0 {
            return false;
        }
        if want_exec && (stat.mode & 0o100) == 0 {
            return false;
        }
        return true;
    }

    if id.is_member(stat.gid, use_effective) {
        if want_read && (stat.mode & 0o040) == 0 {
            return false;
        }
        if want_write && (stat.mode & 0o020) == 0 {
            return false;
        }
        if want_exec && (stat.mode & 0o010) == 0 {
            return false;
        }
        return true;
    }

    if want_read && (stat.mode & 0o004) == 0 {
        return false;
    }
    if want_write && (stat.mode & 0o002) == 0 {
        return false;
    }
    if want_exec && (stat.mode & 0o001) == 0 {
        return false;
    }
    true
}

/// Checked variant returning [`VfsError::PermissionDenied`] on failure.
pub fn check_access_stat(stat: &Stat, mode: u32, use_effective: bool) -> Result<(), VfsError> {
    if can_access_stat(stat, mode, use_effective) {
        Ok(())
    } else {
        Err(VfsError::PermissionDenied)
    }
}
