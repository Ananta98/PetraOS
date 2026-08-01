use crate::proc::process::Process;
use crate::syscall::SyscallResult;
use crate::vm::vma::VmaManager;
use ostd::Error;
use ostd::arch::cpu::context::UserContext;

use super::{Timespec, monotonic_ns, read_timespec, write_timespec};

/// **`nanosleep(const struct timespec *req, struct timespec *rem)`** — SYS 35
///
/// Suspends the calling thread for at least the duration specified by
/// `req`. On early wake-up (signal delivery), the remaining time is
/// written to `rem` when `rem` is non-null.
pub fn syscall_nanosleep(
    arg0: usize, // *req
    arg1: usize, // *rem (nullable)
    _a2: usize,
    _a3: usize,
    _a4: usize,
    _a5: usize,
    vm: &VmaManager,
    _ctx: &mut UserContext,
) -> SyscallResult {
    SyscallResult::from_result((|| {
        let req = read_timespec(vm, arg0)?;

        if req.tv_sec < 0 || req.tv_nsec < 0 || req.tv_nsec >= 1_000_000_000 {
            return Err(Error::InvalidArgs);
        }

        let sleep_ns = req.tv_sec as u64 * 1_000_000_000 + req.tv_nsec as u64;
        if sleep_ns == 0 {
            return Ok(());
        }

        let start_ns = monotonic_ns();
        let deadline_ns = start_ns.saturating_add(sleep_ns);

        let process = Process::current();
        let signals = process.signals.clone();

        loop {
            let now = monotonic_ns();
            if now >= deadline_ns {
                break;
            }

            // Check if any signal was delivered to interrupt nanosleep
            if signals.queue.has_pending() {
                let rem_ns = deadline_ns.saturating_sub(now);
                if arg1 != 0 {
                    let rem_ts = Timespec {
                        tv_sec: (rem_ns / 1_000_000_000) as i64,
                        tv_nsec: (rem_ns % 1_000_000_000) as i64,
                    };
                    let _ = write_timespec(vm, arg1, rem_ts);
                }
                return Err(Error::IoError); // Interrupted by signal (-EINTR)
            }

            core::hint::spin_loop();
        }

        // Write zero remainder — slept full duration.
        if arg1 != 0 {
            write_timespec(
                vm,
                arg1,
                Timespec {
                    tv_sec: 0,
                    tv_nsec: 0,
                },
            )?;
        }

        Ok(())
    })())
}
