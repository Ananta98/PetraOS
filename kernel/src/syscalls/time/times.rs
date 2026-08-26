//! sys_times system call handler.

use super::*;
use crate::syscalls::{SyscallError, SyscallResult, UserPtr};
use crate::arch::syscall::syscall::SyscallFrame;


/// `sys_times` (SYS_TIMES = 100)
/// Returns elapsed system clock ticks since system boot, and fills process CPU timing.
pub fn sys_times(frame: &mut SyscallFrame) -> SyscallResult {
    let buf_ptr = UserPtr::<Tms>::from_u64(frame.arg1());

    // Standard POSIX clock ticks per second (CLK_TCK = 100)
    let elapsed_ns = crate::arch::timer::hpet::elapsed_ns();
    let total_ticks = (elapsed_ns / 10_000_000) as i64; // 10ms per tick (100Hz)

    if !buf_ptr.is_null() {
        // Retrieve current process CPU times if available
        let mut utime = total_ticks;
        let stime = 0i64;

        if let Some(proc_arc) = crate::proc::current_process() {
            let proc = proc_arc.lock();
            // Estimate process CPU ticks from threads' accumulated vruntime
            let mut process_vruntime_ns = 0u64;
            for thread_arc in proc.threads.values() {
                let thread = thread_arc.lock();
                process_vruntime_ns = process_vruntime_ns.saturating_add(thread.vruntime);
            }
            if process_vruntime_ns > 0 {
                utime = (process_vruntime_ns / 10_000_000) as i64;
            }
        }

        let tms = Tms {
            tms_utime: utime,
            tms_stime: stime,
            tms_cutime: 0,
            tms_cstime: 0,
        };

        buf_ptr.write(tms).ok_or(SyscallError::EFAULT)?;
    }

    Ok(total_ticks as usize)
}
