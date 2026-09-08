use alloc::sync::Arc;
use alloc::vec::Vec;

/// Raw syscall value meaning "no change" (`(uid_t)-1`).
pub const ID_NO_CHANGE: u32 = u32::MAX;

/// Maximum number of supplementary groups per process.
pub const MAX_SUPPLEMENTARY_GROUPS: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credentials {
    pub uid: u32,
    pub gid: u32,
    pub euid: u32,
    pub egid: u32,
    pub suid: u32,
    pub sgid: u32,
    pub fsuid: u32,
    pub fsgid: u32,
    pub supplementary_gids: Vec<u32>,
}

impl Credentials {
    /// Default for PID 1 init: privileged root.
    ///
    /// Kept for compatibility. All processes start as root until a
    /// privileged `setuid`/`setgid` transition drops them to another user,
    /// so `getuid` returning 0 initially is correct, not a bug.
    pub fn new() -> Arc<Self> {
        Self::new_root()
    }

    pub fn new_root() -> Arc<Self> {
        Arc::new(Self {
            uid: 0,
            gid: 0,
            euid: 0,
            egid: 0,
            suid: 0,
            sgid: 0,
            fsuid: 0,
            fsgid: 0,
            supplementary_gids: Vec::new(),
        })
    }

    /// Explicit non-root credentials (tests, future user-process spawning).
    pub fn new_with_ids(uid: u32, gid: u32) -> Arc<Self> {
        Arc::new(Self {
            uid,
            gid,
            euid: uid,
            egid: gid,
            suid: uid,
            sgid: gid,
            fsuid: uid,
            fsgid: gid,
            supplementary_gids: Vec::new(),
        })
    }

    pub fn is_privileged(&self) -> bool {
        self.euid == 0
    }

    /// Convert a raw syscall id argument to `None` when it is `(uid_t)-1`.
    pub fn opt_id(raw: u64) -> Option<u32> {
        let v = raw as u32;
        if v == ID_NO_CHANGE {
            None
        } else {
            Some(v)
        }
    }

    fn uid_matches(&self, target: u32) -> bool {
        target == self.uid || target == self.euid || target == self.suid
    }

    fn gid_matches(&self, target: u32) -> bool {
        target == self.gid || target == self.egid || target == self.sgid
    }

    /// POSIX `setuid`: privileged sets all uids, unprivileged may only
    /// switch `euid` between `uid`/`suid`.
    pub fn set_uid(&mut self, target: u32) -> Result<(), &'static str> {
        if self.is_privileged() {
            self.uid = target;
            self.euid = target;
            self.suid = target;
            self.fsuid = target;
            return Ok(());
        }
        if target == self.uid || target == self.suid {
            self.euid = target;
            self.fsuid = target;
            return Ok(());
        }
        Err("EPERM: unprivileged setuid")
    }

    /// POSIX `setgid` mirror of `set_uid`.
    pub fn set_gid(&mut self, target: u32) -> Result<(), &'static str> {
        if self.euid == 0 {
            self.gid = target;
            self.egid = target;
            self.sgid = target;
            self.fsgid = target;
            return Ok(());
        }
        if target == self.gid || target == self.sgid {
            self.egid = target;
            self.fsgid = target;
            return Ok(());
        }
        Err("EPERM: unprivileged setgid")
    }

    pub fn set_reuid(
        &mut self,
        ruid: Option<u32>,
        euid: Option<u32>,
    ) -> Result<(), &'static str> {
        if ruid.is_none() && euid.is_none() {
            return Ok(());
        }
        if self.is_privileged() {
            if let Some(r) = ruid {
                self.uid = r;
            }
            if let Some(e) = euid {
                self.euid = e;
                self.fsuid = e;
            }
            self.suid = self.euid;
            return Ok(());
        }
        if let Some(r) = ruid {
            if !self.uid_matches(r) {
                return Err("EPERM: invalid ruid");
            }
        }
        if let Some(e) = euid {
            if !self.uid_matches(e) {
                return Err("EPERM: invalid euid");
            }
        }
        if let Some(r) = ruid {
            self.uid = r;
        }
        if let Some(e) = euid {
            self.euid = e;
            self.fsuid = e;
        }
        Ok(())
    }

    pub fn set_regid(
        &mut self,
        rgid: Option<u32>,
        egid: Option<u32>,
    ) -> Result<(), &'static str> {
        if rgid.is_none() && egid.is_none() {
            return Ok(());
        }
        if self.euid == 0 {
            if let Some(r) = rgid {
                self.gid = r;
            }
            if let Some(e) = egid {
                self.egid = e;
                self.fsgid = e;
            }
            self.sgid = self.egid;
            return Ok(());
        }
        if let Some(r) = rgid {
            if !self.gid_matches(r) {
                return Err("EPERM: invalid rgid");
            }
        }
        if let Some(e) = egid {
            if !self.gid_matches(e) {
                return Err("EPERM: invalid egid");
            }
        }
        if let Some(r) = rgid {
            self.gid = r;
        }
        if let Some(e) = egid {
            self.egid = e;
            self.fsgid = e;
        }
        Ok(())
    }

    pub fn set_resuid(
        &mut self,
        ruid: Option<u32>,
        euid: Option<u32>,
        suid: Option<u32>,
    ) -> Result<(), &'static str> {
        if ruid.is_none() && euid.is_none() && suid.is_none() {
            return Ok(());
        }
        if !self.is_privileged() {
            if let Some(r) = ruid {
                if !self.uid_matches(r) {
                    return Err("EPERM: invalid resuid ruid");
                }
            }
            if let Some(e) = euid {
                if !self.uid_matches(e) {
                    return Err("EPERM: invalid resuid euid");
                }
            }
            if let Some(s) = suid {
                if !self.uid_matches(s) {
                    return Err("EPERM: invalid resuid suid");
                }
            }
        }
        if let Some(r) = ruid {
            self.uid = r;
        }
        if let Some(e) = euid {
            self.euid = e;
            self.fsuid = e;
        }
        if let Some(s) = suid {
            self.suid = s;
        }
        Ok(())
    }

    pub fn set_resgid(
        &mut self,
        rgid: Option<u32>,
        egid: Option<u32>,
        sgid: Option<u32>,
    ) -> Result<(), &'static str> {
        if rgid.is_none() && egid.is_none() && sgid.is_none() {
            return Ok(());
        }
        if self.euid != 0 {
            if let Some(r) = rgid {
                if !self.gid_matches(r) {
                    return Err("EPERM: invalid resgid rgid");
                }
            }
            if let Some(e) = egid {
                if !self.gid_matches(e) {
                    return Err("EPERM: invalid resgid egid");
                }
            }
            if let Some(s) = sgid {
                if !self.gid_matches(s) {
                    return Err("EPERM: invalid resgid sgid");
                }
            }
        }
        if let Some(r) = rgid {
            self.gid = r;
        }
        if let Some(e) = egid {
            self.egid = e;
            self.fsgid = e;
        }
        if let Some(s) = sgid {
            self.sgid = s;
        }
        Ok(())
    }

    pub fn set_fsuid(&mut self, target: u32) -> Result<u32, &'static str> {
        let prev = self.fsuid;
        if self.is_privileged()
            || target == self.uid
            || target == self.euid
            || target == self.suid
            || target == self.fsuid
        {
            self.fsuid = target;
            return Ok(prev);
        }
        Err("EPERM: invalid fsuid")
    }

    pub fn set_fsgid(&mut self, target: u32) -> Result<u32, &'static str> {
        let prev = self.fsgid;
        if self.euid == 0
            || target == self.gid
            || target == self.egid
            || target == self.sgid
            || target == self.fsgid
        {
            self.fsgid = target;
            return Ok(prev);
        }
        Err("EPERM: invalid fsgid")
    }

    pub fn set_supplementary_gids(&mut self, groups: &[u32]) -> Result<(), &'static str> {
        if !self.is_privileged() {
            return Err("EPERM: only root may call setgroups");
        }
        if groups.len() > MAX_SUPPLEMENTARY_GROUPS {
            return Err("EINVAL: too many supplementary groups");
        }
        self.supplementary_gids.clear();
        for g in groups {
            self.supplementary_gids.push(*g);
        }
        Ok(())
    }
}
