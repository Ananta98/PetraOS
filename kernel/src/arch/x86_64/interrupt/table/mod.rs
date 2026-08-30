//! IDT Interrupt and Exception Handler Tables.
//!
//! Organizes interrupt handlers into distinct subsystems:
//! - [`exceptions`]: x86_64 CPU architectural exception handlers (#DE through #CP).
//! - [`page_fault`]: Page fault (#PF) handler with demand paging and user/kernel separation.
//! - [`ps2`]: PS/2 keyboard controller interrupt handler.
//! - [`timer`]: LAPIC timer and spurious interrupt handlers.

pub mod exceptions;
pub mod page_fault;
pub mod ps2;
pub mod timer;

/// Terminates an offending user-space process on unrecoverable fault and resumes scheduling.
pub(crate) fn kill_user_process(sig: u8) -> ! {
    let ppid_opt = if let Some(proc_arc) = crate::proc::current_process() {
        let mut proc = proc_arc.lock();
        proc.exit(128 + sig as i32);
        proc.ppid
    } else {
        crate::proc::ProcessId(0)
    };

    if let Some(thread_arc) = crate::proc::current_thread() {
        let mut t = thread_arc.lock();
        t.state = crate::proc::ThreadState::Zombie;
        t.exit_code = Some((128 + sig as u32) as u32);
    }

    if ppid_opt.as_u64() > 0 {
        if let Some(parent_arc) = crate::proc::find_process(ppid_opt) {
            let mut parent = parent_arc.lock();
            let _ = parent.send_signal(crate::ipc::signal::SIGCHLD);
        }
    }

    loop {
        crate::sched::schedule(false);
    }
}
