// SPDX-License-Identifier: GPL-2.0

//! Process stop and continue lifecycle state transitions.

use alloc::{sync::Arc, vec::Vec};

use crate::{
    api::signal::{SigInfo, Signal, code},
    proc::{
        ExitStatus, Proc, allproc_find,
        thread::{Thread, ThreadState},
    },
};

/// Linux `CLD_STOPPED` and `CLD_CONTINUED` status codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoppedStatus {
    Stopped,
    Continued,
}

impl StoppedStatus {
    pub const fn into_raw(self) -> i32 {
        match self {
            Self::Stopped => 5,   // CLD_STOPPED
            Self::Continued => 6, // CLD_CONTINUED
        }
    }
}

/// Stops every thread of the process that `td` belongs to.
pub fn stop_thread(td: &Thread, sig: Signal) {
    let Some(proc) = td.proc() else {
        return;
    };

    let threads: Vec<Arc<Thread>> = proc.inner.lock().threads.clone();
    for thread in threads.iter() {
        let mut inner = thread.inner.lock();
        if inner.state == ThreadState::CanRun {
            inner.state = ThreadState::Stopped;
            inner.stopped_by = Some(sig);
        }
    }

    proc.inner.lock().stop_status = Some(ExitStatus::Stopped(sig));

    if let Some(parent) = proc.inner.lock().ppid.and_then(allproc_find) {
        let _ = parent.send_signal(
            SigInfo {
                si_signo: Signal::SIGCHLD.as_u32() as i32,
                si_code: code::USER,
                si_pid: proc.pid.as_u32() as i32,
                si_status: StoppedStatus::Stopped.into_raw(),
                ..SigInfo::default()
            },
            proc.pid.as_u32(),
        );
        parent.child_waitq.wake_all();
        crate::proc::lifecycle::wait::notify_child_events();
    }
}

/// Resumes a stopped process and wakes its threads parked on `stop_queue`.
pub fn continue_thread(proc: &Proc) {
    let threads: Vec<Arc<Thread>> = proc.inner.lock().threads.clone();
    for thread in threads.iter() {
        let mut inner = thread.inner.lock();
        if inner.state == ThreadState::Stopped {
            inner.state = ThreadState::CanRun;
            inner.stopped_by = None;
        }
    }

    proc.inner.lock().stop_status = Some(ExitStatus::Continued);
    proc.stop_queue.wake_all();

    if let Some(parent) = proc.inner.lock().ppid.and_then(allproc_find) {
        let _ = parent.send_signal(
            SigInfo {
                si_signo: Signal::SIGCHLD.as_u32() as i32,
                si_code: code::USER,
                si_pid: proc.pid.as_u32() as i32,
                si_status: StoppedStatus::Continued.into_raw(),
                ..SigInfo::default()
            },
            proc.pid.as_u32(),
        );
        parent.child_waitq.wake_all();
        crate::proc::lifecycle::wait::notify_child_events();
    }
}
