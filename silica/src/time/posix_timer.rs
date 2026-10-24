// SPDX-License-Identifier: GPL-2.0

//! POSIX.1b Interval Timers (`timer_create`, `timer_settime`, etc.).
//!
//! Provides per-process interval timers supporting monotonic, realtime, and CPU
//! clocks, one-shot and periodic intervals, and signal notification to processes
//! or specific threads via `sigevent`.

use alloc::sync::Arc;
use core::time::Duration;
use ostd::sync::SpinLock;

use crate::{
    proc::{
        Pid, Proc, ProcState, allproc_find,
        thread::{Thread, Tid},
    },
    time::{
        CLOCK_PROCESS_CPUTIME_ID, CLOCK_REALTIME, CLOCK_REALTIME_ALARM, CLOCK_TAI,
        CLOCK_THREAD_CPUTIME_ID,
    },
    uapi::signal::{SigInfo, code},
};

pub const SIGEV_SIGNAL: i32 = 0;
pub const SIGEV_NONE: i32 = 1;
pub const SIGEV_THREAD: i32 = 2;
pub const SIGEV_THREAD_ID: i32 = 4;

/// Internal mutable state of a POSIX timer.
pub struct PosixTimerInner {
    pub armed: bool,
    pub interval_ns: u64,
    pub next_deadline_ns: u64,
    pub overrun: i32,
    pub generation: u64,
    pub ktimer: Option<crate::kpi::time::KTimer>,
}

/// A POSIX per-process timer descriptor.
pub struct PosixTimer {
    pub id: i32,
    pub clock_id: i32,
    pub sigev_notify: i32,
    pub sigev_signo: i32,
    pub sigev_value: u64,
    pub sigev_tid: Option<Tid>,
    pub proc_id: Pid,
    pub inner: SpinLock<PosixTimerInner>,
}

impl PosixTimer {
    pub fn new(
        id: i32,
        clock_id: i32,
        sigev_notify: i32,
        sigev_signo: i32,
        sigev_value: u64,
        sigev_tid: Option<Tid>,
        proc_id: Pid,
    ) -> Self {
        Self {
            id,
            clock_id,
            sigev_notify,
            sigev_signo,
            sigev_value,
            sigev_tid,
            proc_id,
            inner: SpinLock::new(PosixTimerInner {
                armed: false,
                interval_ns: 0,
                next_deadline_ns: 0,
                overrun: 0,
                generation: 0,
                ktimer: None,
            }),
        }
    }
}

/// Returns the current timestamp in nanoseconds for the given clock ID.
pub fn get_clock_now(clock_id: i32) -> u64 {
    match clock_id {
        CLOCK_REALTIME | CLOCK_REALTIME_ALARM | CLOCK_TAI => crate::time::get_realtime_ns(),
        CLOCK_PROCESS_CPUTIME_ID | CLOCK_THREAD_CPUTIME_ID => {
            if let Some(td) = Thread::current() {
                crate::sched::stats::for_task(&td.td_kstack)
                    .map(|s| s.exec_ns)
                    .unwrap_or(0)
            } else {
                crate::time::get_monotonic_ns()
            }
        }
        _ => crate::time::get_monotonic_ns(),
    }
}

/// Scheduled callback triggered when a POSIX timer reaches expiry.
pub fn on_timer_expire(timer: Arc<PosixTimer>, proc_id: Pid, generation: u64) {
    let mut inner = timer.inner.lock();
    if inner.generation != generation || !inner.armed {
        return;
    }

    let interval_ns = inner.interval_ns;
    if interval_ns > 0 {
        let clock_now = get_clock_now(timer.clock_id);
        inner.next_deadline_ns = clock_now.saturating_add(interval_ns);
    } else {
        inner.armed = false;
        inner.next_deadline_ns = 0;
    }
    drop(inner);

    // Deliver notification signal if process is still alive
    if let Some(proc) = allproc_find(proc_id) {
        if proc.state() != ProcState::Zombie {
            match timer.sigev_notify {
                SIGEV_SIGNAL => {
                    let siginfo = SigInfo {
                        si_signo: timer.sigev_signo,
                        si_code: code::TIMER,
                        si_addr: timer.id as u64,
                        si_value: timer.sigev_value,
                        ..Default::default()
                    };
                    let _ = proc.send_signal(siginfo, 0);
                }
                SIGEV_THREAD_ID => {
                    if let Some(tid) = timer.sigev_tid {
                        let proc_inner = proc.inner.lock();
                        if let Some(target) =
                            proc_inner.threads.iter().find(|t| t.tid == tid).cloned()
                        {
                            drop(proc_inner);
                            let siginfo = SigInfo {
                                si_signo: timer.sigev_signo,
                                si_code: code::TIMER,
                                si_addr: timer.id as u64,
                                si_value: timer.sigev_value,
                                ..Default::default()
                            };
                            target.post_signal(siginfo);
                        }
                    }
                }
                _ => {} // SIGEV_NONE
            }
        }
    }

    // If periodic and still armed with matching generation, schedule the next iteration
    if interval_ns > 0 {
        let mut inner = timer.inner.lock();
        if inner.generation == generation && inner.armed {
            if let Some(old) = inner.ktimer.take() {
                old.cancel();
            }
            let timer_clone = Arc::clone(&timer);
            let ktimer = crate::kpi::task::schedule_delayed_work_fn(
                move || {
                    on_timer_expire(timer_clone, proc_id, generation);
                },
                Duration::from_nanos(interval_ns),
            );
            inner.ktimer = Some(ktimer);
        }
    } else {
        let mut inner = timer.inner.lock();
        if inner.generation == generation {
            inner.ktimer = None;
        }
    }
}

/// Cleans up all POSIX timers for an exiting or execve-ing process.
pub fn on_proc_exit(proc: &Proc) {
    let mut timers = proc.posix_timers.lock();
    for timer in timers.values() {
        let mut inner = timer.inner.lock();
        inner.armed = false;
        inner.generation = inner.generation.wrapping_add(1);
        if let Some(ktimer) = inner.ktimer.take() {
            ktimer.cancel();
        }
    }
    timers.clear();
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use crate::time::{Itimerspec, Timespec};
    use ostd::prelude::ktest;

    #[ktest]
    fn test_itimerspec_serde() {
        let it = Itimerspec::new(Timespec::new(1, 500_000), Timespec::new(2, 250_000));
        let bytes = it.to_bytes();
        let decoded = Itimerspec::from_bytes(&bytes).expect("decode itimerspec");
        assert_eq!(it, decoded);
    }

    #[ktest]
    fn test_posix_timer_create_and_disarm() {
        let pid = Pid::from_u32(100).unwrap();
        let timer = PosixTimer::new(
            0,
            crate::time::CLOCK_MONOTONIC,
            SIGEV_SIGNAL,
            14,
            42,
            None,
            pid,
        );
        assert_eq!(timer.id, 0);
        assert_eq!(timer.clock_id, crate::time::CLOCK_MONOTONIC);
        assert_eq!(timer.sigev_signo, 14);
        assert_eq!(timer.sigev_value, 42);

        let mut inner = timer.inner.lock();
        assert!(!inner.armed);
        inner.armed = true;
        inner.interval_ns = 1_000_000;
        inner.generation += 1;
        assert!(inner.armed);
        inner.armed = false;
        assert!(!inner.armed);
    }
}
