/// `tgkill(tgid, tid, sig)` — send a signal to a specific thread in a
/// process (SYS_tgkill = 234).
///
/// Linux semantics:
/// - `tgid` is the thread-group leader PID (the process PID).
/// - `tid`  is the target thread ID.
/// - `sig`  is the signal to deliver.
///
/// In PetraOS the signal is delivered to the *process* identified by `tgid`.
/// Per-thread signal delivery (routing to a specific TID) is a future
/// extension once threads carry their own `SigQueue`.
///
/// Returns `0` on success, negated `errno` on failure.
use crate::ipc::dispatch::send_signal_to_pid;
use crate::proc::pid_table::{PROCESS_TABLE, Pid};
use crate::proc::process::Process;
use crate::syscall::{SyscallResult, to_continue_unit};
use crate::vm::vma::VmaManager;
use ostd::Error;

/// System call entry: `tgkill(tgid, tid, sig)`.
pub(crate) fn syscall_tgkill(
    arg0: usize, // pid_t tgid
    arg1: usize, // pid_t tid (currently ignored — signal goes to process)
    arg2: usize, // int sig
    _: usize,
    _: usize,
    _: usize,
    _: &VmaManager,
    _: &mut ostd::arch::cpu::context::UserContext,
) -> SyscallResult {
    let tgid = arg0 as u32;
    let _tid = arg1 as u32; // reserved for per-thread routing
    let signum = arg2 as u32;

    if signum > crate::ipc::SIGRTMAX && signum != 0 {
        return to_continue_unit(Err(Error::InvalidArgs));
    }

    let target = Pid::from_raw(tgid);
    if PROCESS_TABLE.get_process(target).is_none() {
        return to_continue_unit(Err(Error::InvalidArgs));
    }

    if signum == 0 {
        // Validity check only.
        return to_continue_unit(Ok(()));
    }

    let sender_pid = Process::current().pid.as_u32();
    to_continue_unit(send_signal_to_pid(target, signum, sender_pid))
}
