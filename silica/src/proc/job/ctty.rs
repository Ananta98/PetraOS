// SPDX-License-Identifier: GPL-2.0

//! Controlling terminal associations and orphan process group management.

use crate::{
    api::signal::Signal,
    proc::{
        Pid,
        types::table::{allproc_find, pgrp_members},
    },
};

/// Determines whether a process group is orphaned according to POSIX rules.
///
/// An orphaned process group is one where every member's parent is either:
/// 1. A member of the same process group, or
/// 2. Not a member of the same session.
///
/// If any member has a parent that is in the same session but in a different process group,
/// the process group is not orphaned.
pub fn is_orphaned_pgrp(pgrp: Pid) -> bool {
    let members = pgrp_members(pgrp);
    for member in &members {
        if let Some(ppid) = member.ppid() {
            if let Some(parent) = allproc_find(ppid) {
                // Parent is in the same session, but different process group: group is active (not orphaned)
                if parent.sid() == member.sid() && parent.pgid() != member.pgid() {
                    return false;
                }
            }
        }
    }
    true
}

/// Checks if an orphaned process group contains stopped processes and, if so, delivers
/// `SIGHUP` followed by `SIGCONT` to all group members (POSIX requirement).
pub fn notify_orphaned_pgrp_if_stopped(pgrp: Pid) {
    if !is_orphaned_pgrp(pgrp) {
        return;
    }

    let members = pgrp_members(pgrp);
    let any_stopped = members.iter().any(|p| {
        p.inner.lock().stop_status.is_some()
            || p.inner.lock().threads.iter().any(|t| {
                t.inner.lock().state == crate::proc::thread::ThreadState::Stopped
            })
    });

    if any_stopped {
        for p in &members {
            p.post_signal(Signal::SIGHUP);
            p.post_signal(Signal::SIGCONT);
        }
    }
}
