// SPDX-License-Identifier: GPL-2.0

//! Time and clock related system calls.

pub mod clock;
pub mod gettimeofday;
pub mod nanosleep;
pub mod timer;
pub mod times;

pub use clock::*;
pub use gettimeofday::*;
pub use nanosleep::*;
pub use timer::*;
pub use times::*;

// Re-export common time types and helper functions for backward compatibility
pub use crate::time::{Itimerval, Timespec, Timeval, get_boot_epoch_secs};

/// Sleeps until `now_fn()` reaches `target_ns`, re-arming the one-shot jiffy
/// timer if it fires slightly before the nanosecond clock reaches `target_ns`.
pub(super) fn sleep_until_ns<F: Fn() -> u64>(
    target_ns: u64,
    now_fn: F,
) -> crate::uapi::errno::Result<()> {
    let (waiter, _) = ostd::sync::Waiter::new_pair();
    let cancel = || {
        if crate::proc::Thread::should_interrupt() {
            Err(crate::uapi::errno::Errno::EINTR)
        } else {
            Ok(())
        }
    };

    loop {
        let now_ns = now_fn();
        if now_ns >= target_ns {
            return Ok(());
        }
        let fired = alloc::sync::Arc::new(core::sync::atomic::AtomicBool::new(false));
        let fired_cb = alloc::sync::Arc::clone(&fired);
        let waker = waiter.waker();
        let ktimer = crate::kpi::time::arm(
            core::time::Duration::from_nanos(target_ns - now_ns),
            Some(alloc::boxed::Box::new(move || {
                fired_cb.store(true, core::sync::atomic::Ordering::Release);
                waker.wake_up();
            })),
        );

        let cond = || {
            if now_fn() >= target_ns || fired.load(core::sync::atomic::Ordering::Acquire) {
                Some(())
            } else {
                None
            }
        };

        let wait_res = waiter.wait_until_or_cancelled(cond, cancel);
        ktimer.cancel();
        wait_res?;
    }
}
