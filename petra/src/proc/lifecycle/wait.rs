// SPDX-License-Identifier: GPL-2.0

//! Process wait and status observation.
//!
//! Implements `kernel_wait` for observing child process state changes (termination,
//! stopping, continuation) and reaping zombies.

use alloc::{sync::Arc, vec::Vec};
use bitflags::bitflags;

use crate::{
    uapi::errno::Result,
    proc::{
        ExitStatus, Proc, ProcState,
        lifecycle::exit::proc_reap,
        types::{pid::Pid, table::allproc_find},
    },
};

bitflags! {
    /// Options for `wait6(2)`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
    pub struct WaitOptions: u32 {
        /// Do not block if no child has exited.
        const WNOHANG    = 1 << 0;
        /// Wait for processes that have exited.
        const WEXITED    = 1 << 1;
        /// Wait for processes that have stopped.
        const WSTOPPED   = 1 << 2;
        /// Wait for processes that have continued.
        const WCONTINUED = 1 << 3;
        /// Keep the process in a waitable state (do not reap).
        const WNOWAIT    = 1 << 4;
    }
}

/// Identification target type for `wait6(2)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdType {
    /// Any child process.
    All,
    /// Specific child PID.
    Pid(Pid),
    /// Process group.
    Pgid(u32),
}

/// Wait result payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaitResult {
    pub pid: Pid,
    pub status: ExitStatus,
    pub exit_code: i32,
    pub uid: u32,
}

/// Compatibility hook for child state change notification.
#[inline]
pub fn notify_child_events() {
    // Child status events wake the parent's specific `child_waitq` directly.
}

/// Collects the children of `parent` that `idtype` selects.
fn child_candidates(parent: &Arc<Proc>, idtype: IdType) -> Result<Vec<Pid>> {
    let children = parent.inner.lock().children.clone();
    if children.is_empty() {
        crate::return_errno!(ECHILD, "no child processes exist");
    }

    let candidates: Vec<Pid> = children
        .into_iter()
        .filter(|&child_pid| match idtype {
            IdType::All => true,
            IdType::Pid(target) => child_pid == target,
            IdType::Pgid(target_pgid) => {
                allproc_find(child_pid).is_some_and(|c| c.pgid().as_u32() == target_pgid)
            }
        })
        .collect();

    if candidates.is_empty() {
        crate::return_errno!(ECHILD, "no matching child process found");
    }

    Ok(candidates)
}

/// waits for child status changes and optionally reaps zombies.
pub fn kernel_wait(
    parent: &Arc<Proc>,
    idtype: IdType,
    options: WaitOptions,
) -> Result<Option<WaitResult>> {
    // `WNOHANG` and `WNOWAIT` are modifiers, not state-change selectors. POSIX
    // leaves the set empty when a caller passes only modifiers, and every real
    // caller means "exited", so default to that instead of treating the request as
    // unsatisfiable -- which is what made `WNOHANG` alone never observe an exit.
    let options = if options
        .intersects(WaitOptions::WEXITED | WaitOptions::WSTOPPED | WaitOptions::WCONTINUED)
    {
        options
    } else {
        options | WaitOptions::WEXITED
    };

    let mut candidates = child_candidates(parent, idtype)?;
    loop {
        // Check for eligible children
        for &child_pid in &candidates {
            if let Some(child) = allproc_find(child_pid) {
                let (state, status, stop_status) = {
                    let inner = child.inner.lock();
                    (inner.state, inner.xstat, inner.stop_status)
                };

                // Check stopped status
                if let Some(ExitStatus::Stopped(sig)) = stop_status
                    && options.contains(WaitOptions::WSTOPPED)
                {
                    if !options.contains(WaitOptions::WNOWAIT) {
                        child.inner.lock().stop_status = None;
                    }
                    let uid = child.cred().cr_uid.as_u32();
                    return Ok(Some(WaitResult {
                        pid: child_pid,
                        status: ExitStatus::Stopped(sig),
                        exit_code: (sig.as_u32() as i32 & 0xff) << 8 | 0x7f,
                        uid,
                    }));
                }

                // Check continued status
                if let Some(ExitStatus::Continued) = stop_status
                    && options.contains(WaitOptions::WCONTINUED)
                {
                    if !options.contains(WaitOptions::WNOWAIT) {
                        child.inner.lock().stop_status = None;
                    }
                    let uid = child.cred().cr_uid.as_u32();
                    return Ok(Some(WaitResult {
                        pid: child_pid,
                        status: ExitStatus::Continued,
                        exit_code: 0xffff,
                        uid,
                    }));
                }

                // Check exited / zombie status
                if state == ProcState::Zombie && options.contains(WaitOptions::WEXITED) {
                    let uid = child.cred().cr_uid.as_u32();
                    let final_status = if options.contains(WaitOptions::WNOWAIT) {
                        status.unwrap_or(ExitStatus::Exited(0))
                    } else {
                        let (_, s) = proc_reap(parent, child_pid)?;
                        s
                    };

                    let exit_code = match final_status {
                        ExitStatus::Exited(c) => c,
                        ExitStatus::Signaled { signal, .. } => 128 + signal.as_u32() as i32,
                        ExitStatus::Stopped(signal) => signal.as_u32() as i32,
                        ExitStatus::Continued => 0,
                    };

                    return Ok(Some(WaitResult {
                        pid: child_pid,
                        status: final_status,
                        exit_code,
                        uid,
                    }));
                }
            }
        }

        // If non-blocking was requested, return Ok(None) immediately
        if options.contains(WaitOptions::WNOHANG) {
            return Ok(None);
        }

        // Wait on parent's child wait queue until notified of child state change,
        // then rebuild candidate list.
        let check_event = || {
            candidates.iter().any(|&child_pid| {
                if let Some(c) = allproc_find(child_pid) {
                    let inner = c.inner.lock();
                    if inner.state == ProcState::Zombie && options.contains(WaitOptions::WEXITED) {
                        return true;
                    }
                    if matches!(inner.stop_status, Some(ExitStatus::Stopped(_)))
                        && options.contains(WaitOptions::WSTOPPED)
                    {
                        return true;
                    }
                    if matches!(inner.stop_status, Some(ExitStatus::Continued))
                        && options.contains(WaitOptions::WCONTINUED)
                    {
                        return true;
                    }
                }
                false
            })
        };

        parent
            .child_waitq
            .wait_until(|| if check_event() { Some(()) } else { None });
        candidates = child_candidates(parent, idtype)?;
    }
}
