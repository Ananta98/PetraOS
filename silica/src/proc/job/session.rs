// SPDX-License-Identifier: GPL-2.0

//! POSIX Session management (`setsid`, `getsid`, session leader validation).

use crate::{
    api::errno::{Errno, Result},
    proc::{
        Pid, Proc,
        types::table::PROCTREE_LOCK,
    },
};

impl Proc {
    /// Puts this process in a new session and process group with itself as leader.
    ///
    /// Returns the new session identifier, or `EPERM` if the caller is already a
    /// process group leader (`pid == pgid`).
    pub fn setsid(&self) -> Result<Pid> {
        if self.pgid() == self.pid {
            return Err(Errno::EPERM);
        }
        let sid = self.pid;
        let old_pgid = self.pgid();
        let old_sid = self.sid();

        {
            let mut inner = self.inner.lock();
            inner.sid = Some(sid);
            inner.pgid = Some(sid);
        }

        let mut tree = PROCTREE_LOCK.write();
        tree.update_pgid(self.pid, old_pgid, sid);
        tree.update_sid(self.pid, old_sid, sid);
        Ok(sid)
    }

    /// Returns the session ID of this process.
    #[inline]
    pub fn getsid(&self) -> Pid {
        self.sid()
    }

    /// Checks if this process is a session leader (`pid == sid`).
    #[inline]
    pub fn is_session_leader(&self) -> bool {
        self.sid() == self.pid
    }
}
