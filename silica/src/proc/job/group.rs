// SPDX-License-Identifier: GPL-2.0

//! Process group tracking and `setpgid` semantics.

use crate::{
    api::errno::{Errno, Result},
    proc::{Pid, Proc, allproc_find, thread::Thread, types::table::PROCTREE_LOCK},
};

impl Proc {
    /// Moves this process into the group `pgid`.
    ///
    /// POSIX rules:
    /// - A process may only change its own group or that of a child process.
    /// - A session leader cannot change its process group ID (`EPERM`).
    /// - Target group must belong to the same session (`EPERM`).
    pub fn set_pgid(&self, pgid: Pid) -> Result<()> {
        let caller = Thread::proc_or_esrch()?;

        // A session leader cannot change its PGID
        if self.is_session_leader() {
            return Err(Errno::EPERM);
        }

        // Caller must be self or a direct parent
        if caller.pid != self.pid && !caller.is_parent_of(self.pid) {
            return Err(Errno::EPERM);
        }

        // If creating a new group (pgid == self.pid), it belongs to self.sid()
        // If joining an existing group, the group leader must exist in the same session
        if pgid != self.pid {
            let leader = allproc_find(pgid).ok_or(Errno::EPERM)?;
            if leader.sid() != self.sid() {
                return Err(Errno::EPERM);
            }
        }

        let old_pgid = self.pgid();
        self.inner.lock().pgid = Some(pgid);
        PROCTREE_LOCK.write().update_pgid(self.pid, old_pgid, pgid);
        Ok(())
    }

    /// Returns the process group ID of this process.
    #[inline]
    pub fn get_pgid(&self) -> Pid {
        self.pgid()
    }

    /// Checks if this process is the leader of its process group (`pid == pgid`).
    #[inline]
    pub fn is_group_leader(&self) -> bool {
        self.pgid() == self.pid
    }

    /// Checks if this process is the direct parent of `child`.
    #[inline]
    pub fn is_parent_of(&self, child: Pid) -> bool {
        self.inner.lock().children.contains(&child)
    }
}
