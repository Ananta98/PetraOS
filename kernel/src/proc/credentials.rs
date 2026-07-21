use ostd::sync::SpinLock;

// ---------------------------------------------------------------------------
// Credentials
// ---------------------------------------------------------------------------

/// Unix process credentials — the set of user and group IDs that govern
/// permission checks for a process.
///
/// Mirrors the Linux `struct cred` / POSIX credential set:
///
/// | Field   | Meaning                                                   |
/// |---------|-----------------------------------------------------------|
/// | `uid`   | Real user ID — the user that *owns* the process.         |
/// | `euid`  | Effective user ID — used for permission checks.          |
/// | `suid`  | Saved set-user-ID — allows privilege dropping/regaining. |
/// | `fsuid` | Filesystem user ID — used for VFS permission checks.     |
/// | `gid`   | Real group ID — the group that *owns* the process.       |
/// | `egid`  | Effective group ID — used for permission checks.         |
/// | `sgid`  | Saved set-group-ID.                                      |
/// | `fsgid` | Filesystem group ID — used for VFS permission checks.    |
///
/// All fields are kept inside a `SpinLock` so that credential updates (e.g.
/// from `setuid`, `setgid`, or `execve` of a set-UID binary) are
/// race-free even when multiple threads share the same `Process`.

/// The raw credential values, protected by the outer [`Credentials`] lock.
#[derive(Debug, Clone)]
pub struct Credentials {
    /// Real user ID.
    uid: u32,
    /// Effective user ID.
    euid: u32,
    /// Saved set-user-ID.
    suid: u32,
    /// Filesystem user ID.
    fsuid: u32,
    /// Real group ID.
    gid: u32,
    /// Effective group ID.
    egid: u32,
    /// Saved set-group-ID.
    sgid: u32,
    /// Filesystem group ID.
    fsgid: u32,
}

impl Credentials {
    /// Create credentials for a privileged (root) process.
    ///
    /// All UID and GID fields are initialised to `0`.
    pub fn new_root() -> Self {
        Self::new(0, 0)
    }

    /// Create credentials with the given real UID and GID.
    ///
    /// The effective, saved, and filesystem IDs are all initialised to the
    /// same values as the real IDs, which is the correct POSIX starting
    /// point for a freshly spawned process.
    pub fn new(uid: u32, gid: u32) -> Self {
        Self {
            uid,
            euid: uid,
            suid: uid,
            fsuid: uid,
            gid,
            egid: gid,
            sgid: gid,
            fsgid: gid,
        }
    }

    // -----------------------------------------------------------------------
    // Getters
    // -----------------------------------------------------------------------

    /// Returns the real user ID.
    pub fn uid(&self) -> u32 {
        self.uid
    }

    /// Returns the effective user ID.
    pub fn euid(&self) -> u32 {
        self.euid
    }

    /// Returns the saved set-user-ID.
    pub fn suid(&self) -> u32 {
        self.suid
    }

    /// Returns the filesystem user ID.
    pub fn fsuid(&self) -> u32 {
        self.fsuid
    }

    /// Returns the real group ID.
    pub fn gid(&self) -> u32 {
        self.gid
    }

    /// Returns the effective group ID.
    pub fn egid(&self) -> u32 {
        self.egid
    }

    /// Returns the saved set-group-ID.
    pub fn sgid(&self) -> u32 {
        self.sgid
    }

    /// Returns the filesystem group ID.
    pub fn fsgid(&self) -> u32 {
        self.fsgid
    }

    // -----------------------------------------------------------------------
    // Setters
    // -----------------------------------------------------------------------

    /// Sets the real user ID.
    pub fn set_uid(&mut self, uid: u32) {
        self.uid = uid;
    }

    /// Sets the effective user ID.
    pub fn set_euid(&mut self, euid: u32) {
        self.euid = euid;
    }

    /// Sets the saved set-user-ID.
    pub fn set_suid(&mut self, suid: u32) {
        self.suid = suid;
    }

    /// Sets the filesystem user ID.
    pub fn set_fsuid(&mut self, fsuid: u32) {
        self.fsuid = fsuid;
    }

    /// Sets the real group ID.
    pub fn set_gid(&mut self, gid: u32) {
        self.gid = gid;
    }

    /// Sets the effective group ID.
    pub fn set_egid(&mut self, egid: u32) {
        self.egid = egid;
    }

    /// Sets the saved set-group-ID.
    pub fn set_sgid(&mut self, sgid: u32) {
        self.sgid = sgid;
    }

    /// Sets the filesystem group ID.
    pub fn set_fsgid(&mut self, fsgid: u32) {
        self.fsgid = fsgid;
    }

    // -----------------------------------------------------------------------
    // Bulk operations
    // -----------------------------------------------------------------------

    /// Returns `true` if the process is running with root effective privileges
    /// (i.e. `euid == 0`).
    pub fn is_privileged(&self) -> bool {
        self.euid == 0
    }
}
