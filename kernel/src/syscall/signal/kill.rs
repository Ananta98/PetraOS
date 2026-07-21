/// `kill(pid, sig)` — send a signal to a process (SYS_kill = 62).
///
/// Mirrors the POSIX `kill(2)` specification:
/// - `pid > 0`  → send to the process with that PID.
/// - `pid == 0` → send to every process in the calling process's process group
///               (simplified: sends to the calling process itself in PetraOS).
/// - `pid == -1`→ send to every process the caller has permission to signal
///               (not yet implemented; returns `-EINVAL`).
/// - `pid < -1` → send to the process group whose PGID equals `|pid|`.
/// - `sig == 0` → validity check only (no signal sent; `Ok` if target exists).
///
/// Returns `0` on success, or a negated `errno` on failure.
use crate::ipc::dispatch::send_signal_to_pid;
use crate::proc::pid_table::{PROCESS_TABLE, Pid};
use crate::proc::process::Process;
use crate::syscall::{SyscallResult, to_continue_unit};
use crate::vm::vma::VmaManager;
use ostd::Error;

/// System call entry: `kill(pid, sig)`.
pub fn syscall_kill(
    arg0: usize, // pid_t pid (as i32 cast to usize)
    arg1: usize, // int sig
    _: usize,
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    let pid_raw = arg0 as isize;
    let signum = arg1 as u32;
    let sender = Process::current();
    let sender_pid = sender.pid.as_u32();

    if signum > crate::ipc::SIGRTMAX && signum != 0 {
        return to_continue_unit(Err(Error::InvalidArgs));
    }

    if pid_raw > 0 {
        let target = Pid::from_raw(pid_raw as u32);
        if signum == 0 {
            // Validity check: does the process exist?
            if PROCESS_TABLE.get_process(target).is_none() {
                return to_continue_unit(Err(Error::InvalidArgs));
            }
            return to_continue_unit(Ok(()));
        }
        to_continue_unit(send_signal_to_pid(target, signum, sender_pid))
    } else if pid_raw == 0 {
        let pgid = sender.pgid();
        if signum == 0 {
            if PROCESS_TABLE.get_processes_by_pgid(pgid).is_empty() {
                return to_continue_unit(Err(Error::InvalidArgs));
            }
            return to_continue_unit(Ok(()));
        }
        to_continue_unit(crate::ipc::dispatch::send_signal_to_group(
            pgid.as_u32(),
            signum,
            sender_pid,
        ))
    } else if pid_raw < -1 {
        let pgid = (-pid_raw) as u32;
        if signum == 0 {
            if PROCESS_TABLE
                .get_processes_by_pgid(Pid::from_raw(pgid))
                .is_empty()
            {
                return to_continue_unit(Err(Error::InvalidArgs));
            }
            return to_continue_unit(Ok(()));
        }
        to_continue_unit(crate::ipc::dispatch::send_signal_to_group(
            pgid, signum, sender_pid,
        ))
    } else {
        // pid == -1: broadcast — not yet implemented.
        to_continue_unit(Err(Error::InvalidArgs))
    }
}
