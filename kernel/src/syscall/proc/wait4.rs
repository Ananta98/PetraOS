/// `wait4(pid, wstatus, options, rusage)` — wait for process to change state (SYS_wait4 = 61).
///
/// Waits for a child process of the calling process to terminate, stop, or
/// continue, and reaps it.
///
/// Returns the PID of the reaped child on success, `0` if `WNOHANG` was passed
/// and no child has changed state, or a negated `errno` on failure.
use crate::proc::pid_table::Pid;
use crate::proc::process::Process;
use crate::syscall::{SyscallResult, to_continue_i32};
use crate::vm::vma::VmaManager;
use ostd::Error;
use ostd::arch::cpu::context::UserContext;

pub(crate) fn syscall_wait4(
    arg0: usize, // pid_t pid
    arg1: usize, // int *wstatus
    arg2: usize, // int options
    _: usize,    // struct rusage *rusage (ignored)
    _: usize,
    _: usize,
    vm: &VmaManager,
    _: &mut UserContext,
) -> SyscallResult {
    let pid_raw = arg0 as isize;
    let wstatus_ptr = arg1;
    let options = arg2;

    let current_process = Process::current();

    loop {
        // Check if the process has any children. If none, return ECHILD.
        let has_children = {
            let children = current_process.children.lock();
            !children.is_empty()
        };

        if !has_children {
            return to_continue_i32(Err(Error::InvalidArgs)); // ECHILD
        }

        // Try to reap a child.
        let target_pid = if pid_raw == -1 {
            None
        } else {
            Some(Pid::from_raw(pid_raw as u32))
        };

        if let Some((reaped_pid, code)) = current_process.wait_child(target_pid) {
            // Write exit status if wstatus is provided.
            if wstatus_ptr != 0 {
                let status_val = (code & 0xff) << 8;
                let status_bytes = status_val.to_ne_bytes();
                if let Err(err) = vm.copy_to_user(wstatus_ptr, &status_bytes) {
                    return to_continue_i32(Err(err));
                }
            }
            return to_continue_i32(Ok(reaped_pid.as_u32() as i32));
        }

        // If WNOHANG is set, return 0 immediately.
        if (options & 1) != 0 {
            return to_continue_i32(Ok(0));
        }

        // Otherwise yield / spin_loop.
        core::hint::spin_loop();
    }
}
