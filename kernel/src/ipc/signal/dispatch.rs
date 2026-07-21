use super::ProcessSignals;
use super::table::SigHandlerKind;
/// Signal Dispatch — Dequeue and Act on Pending Signals
///
/// `dispatch_pending` is the central function called at safe kernel-to-user
/// transition points (e.g., before returning from a system call or at a
/// scheduler preemption site) to process any unblocked pending signals for the
/// given process.
///
/// # Delivery model
///
/// For each unblocked pending signal the dispatcher:
///
/// 1. Dequeues one `SigInfo` from the process's [`SigQueue`].
/// 2. Looks up the installed [`SigHandlerKind`] in the process's [`SigTable`].
/// 3. Acts:
///    - **Ignore** — discard and continue.
///    - **KernelHandler** — invoke the in-kernel callback immediately.
///    - **Default** — apply the POSIX default action (terminate, stop, …).
///    - **UserHandler** — record the pending user-mode delivery for the
///      architecture signal trampoline; stop dispatching (trampoline will
///      call the handler and then re-enter the kernel via `sys_sigreturn`).
///
/// # Unimplemented: user-space trampoline
///
/// Full user-space handler invocation requires setting up a signal frame on
/// the user stack, adjusting the saved register state, and jumping to a
/// trampoline.  That work belongs to the architecture-specific user-mode layer
/// (`proc::user`) and will be wired up when `sys_sigaction` / `sys_sigreturn`
/// syscalls are added.  For now, `UserHandler` signals are noted (returned to
/// the caller) so the future trampoline can act on them.
use super::types::{DefaultAction, SigInfo, default_action};
use crate::proc::pid_table::PROCESS_TABLE;
use crate::proc::pid_table::Pid;
use crate::proc::process::{Process, ProcessState};
use alloc::sync::Arc;

// ──────────────────────────────────────────────────────────────
// Dispatch result
// ──────────────────────────────────────────────────────────────

/// The outcome of one call to [`dispatch_pending`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchOutcome {
    /// All pending signals were consumed; the process may continue normally.
    Delivered,
    /// A signal with a user-space handler was found.  The caller (trampoline /
    /// syscall return path) should set up the signal frame for this signal.
    PendingUserHandler {
        /// Signal number.
        signum: u32,
        /// Virtual address of the user-space handler function.
        handler_address: usize,
    },
    /// The process was terminated by a signal (SIGKILL or unhandled fatal signal).
    Terminated { signum: u32 },
    /// The process was stopped by SIGSTOP / SIGTSTP.
    Stopped { signum: u32 },
    /// No unblocked signals were pending.
    None,
}

// ──────────────────────────────────────────────────────────────
// Main dispatcher
// ──────────────────────────────────────────────────────────────

/// Process all unblocked pending signals for `process`.
///
/// Must be called from a kernel context where it is safe to mutate process
/// state (e.g., on the syscall return path or after a scheduler preemption).
///
/// Returns the first interesting [`DispatchOutcome`]:
/// - `None` if there was nothing to do.
/// - `Terminated` / `Stopped` if the process was killed/stopped.
/// - `PendingUserHandler` if a user-space handler needs the trampoline.
/// - `Delivered` after all kernel-handled signals are consumed.
pub fn dispatch_pending(process: &mut Process) -> DispatchOutcome {
    // Obtain an Arc handle to signal state that is independent of the
    // &mut Process borrow, allowing us to call process.wake_up() etc.
    let signals: Arc<ProcessSignals> = process.signals.clone();

    loop {
        let info = match signals.queue.dequeue() {
            Some(info) => info,
            None => return DispatchOutcome::None,
        };

        let signum = info.signum;
        let handler_kind = signals.table.get_handler_kind(signum);

        match handler_kind {
            // ── Ignored ───────────────────────────────────────────────────
            SigHandlerKind::Ignore => {
                // Discard and check next pending signal.
                continue;
            }

            // ── Kernel handler ────────────────────────────────────────────
            SigHandlerKind::KernelHandler => {
                signals.table.call_kernel_handler(signum);
                // Continue dispatching remaining signals.
                continue;
            }

            // ── User-space handler ────────────────────────────────────────
            SigHandlerKind::UserHandler(address) => {
                // Block the handler mask + the signal itself while the
                // handler runs (re-entrancy prevention).
                let mut extra_mask = signals.table.handler_mask(signum);
                extra_mask.add(signum);
                signals.queue.block(extra_mask);

                // Return to the caller so the trampoline can set up the
                // signal frame and jump to user space.
                return DispatchOutcome::PendingUserHandler {
                    signum,
                    handler_address: address,
                };
            }

            // ── Default action ────────────────────────────────────────────
            SigHandlerKind::Default => {
                match default_action(signum) {
                    DefaultAction::Ignore => continue,

                    DefaultAction::Continue => {
                        // Wake the process if it was stopped.
                        process.wake_up();
                        continue;
                    }

                    DefaultAction::Stop => {
                        process.set_sleeping();
                        // Notify parent with SIGCHLD.
                        notify_parent_sigchld(process.ppid.clone(), process.pid);
                        return DispatchOutcome::Stopped { signum };
                    }

                    DefaultAction::Terminate | DefaultAction::CoreDump => {
                        process.exit(-(signum as i32));
                        return DispatchOutcome::Terminated { signum };
                    }
                }
            }
        }
    }
}

// ──────────────────────────────────────────────────────────────
// Helper: send SIGCHLD to the parent
// ──────────────────────────────────────────────────────────────

/// Send `SIGCHLD` to the parent process when a child is stopped or terminated.
///
/// This is the kernel's internal path for generating SIGCHLD; the user-facing
/// `kill()` path goes through [`send_signal_to_pid`].
fn notify_parent_sigchld(ppid: Option<Arc<Process>>, child_pid: Pid) {
    let parent_pid = match ppid {
        Some(p) => p.pid,
        None => return,
    };
    let info = SigInfo {
        signum: super::types::SIGCHLD,
        sender_pid: child_pid.as_u32(),
        code: 1, // CLD_STOPPED
    };
    if let Some(parent) = PROCESS_TABLE.get_process(parent_pid) {
        parent.signals.queue.enqueue(info);
    }
}

// ──────────────────────────────────────────────────────────────
// Public: send a signal to a process by PID
// ──────────────────────────────────────────────────────────────

/// Deliver signal `signum` to the process identified by `target_pid`.
///
/// `sender_pid` is the PID of the sender (0 for kernel-generated signals).
///
/// Returns `Ok(())` if the signal was enqueued, or
/// `Err(ostd::Error::InvalidArgs)` if no process with `target_pid` exists or
/// the signal number is invalid.
pub fn send_signal_to_pid(
    target_pid: Pid,
    signum: u32,
    sender_pid: u32,
) -> Result<(), ostd::Error> {
    if signum == 0 || signum > super::types::SIGRTMAX {
        return Err(ostd::Error::InvalidArgs);
    }
    let process = PROCESS_TABLE
        .get_process(target_pid)
        .ok_or(ostd::Error::InvalidArgs)?;
    let info = SigInfo::user(signum, sender_pid);
    process.signals.queue.enqueue(info);
    Ok(())
}

/// Deliver signal `signum` to every process in the process group with PGID
/// equal to `pgid`.
pub fn send_signal_to_group(pgid: u32, signum: u32, sender_pid: u32) -> Result<(), ostd::Error> {
    if signum == 0 || signum > super::types::SIGRTMAX {
        return Err(ostd::Error::InvalidArgs);
    }
    let target_pgid = Pid::from_raw(pgid);
    let processes = PROCESS_TABLE.get_processes_by_pgid(target_pgid);
    if processes.is_empty() {
        return Err(ostd::Error::InvalidArgs);
    }
    for proc in processes {
        let info = SigInfo::user(signum, sender_pid);
        proc.signals.queue.enqueue(info);
    }
    Ok(())
}
