//! Specialized CPU Fault Handlers.
//!
//! Contains handlers for page faults, double faults, and general protection
//! faults. User-space faults terminate the offending process; kernel-space
//! faults trigger a panic with full diagnostics.

use crate::arch::idt::InterruptStackFrame;
use crate::arch::read_cr2;
use crate::mm::{PageFaultErrorCode, VirtAddr};

/// Terminates an offending user-space process on an unrecoverable fault
/// and resumes scheduling.
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

/// Page Fault (#PF) handler.
///
/// Attempts demand-paging / CoW resolution first. On failure, terminates user
/// processes with `SIGSEGV` or panics for kernel faults.
pub extern "C" fn handle_page_fault(stack_frame: &mut InterruptStackFrame, error_code: u64) {
    let fault_virt = VirtAddr::new(read_cr2());
    let fault_code = PageFaultErrorCode::from_bits_truncate(error_code);

    let cpu_id = unsafe { crate::arch::interrupt::lapic::get_lapic().id() };
    let current_thread = crate::sched::current_thread_on_cpu(cpu_id);

    let mut fault_err = None;
    if let Some(thread_arc) = current_thread {
        let thread = thread_arc.lock();
        if let Some(proc_arc) = thread.process.upgrade() {
            let proc = proc_arc.lock();
            let mut addr_space = proc.address_space.lock();
            match addr_space.handle_page_fault(fault_virt, fault_code) {
                Ok(()) => return,
                Err(e) => fault_err = Some(e),
            }
        }
    }

    if (stack_frame.code_segment & 3) == 3
        || fault_virt.as_u64() <= crate::syscalls::USER_SPACE_MAX_ADDR
    {
        let pid = crate::proc::current_process()
            .map(|p| p.lock().pid.as_u64())
            .unwrap_or(0);
        let comm = crate::proc::current_process()
            .map(|p| p.lock().cmdline.args.first().cloned().unwrap_or_default())
            .unwrap_or_default();
        log::warn!(
            "User process page fault (SIGSEGV) PID {} comm '{}' at {:#x}, \
             Error Code: {:#x} [{:?}], RIP={:#x}, CS={:#x}, RSP={:#x}, Reason: {:?}",
            pid,
            comm,
            fault_virt.as_u64(),
            error_code,
            fault_code,
            stack_frame.instruction_pointer,
            stack_frame.code_segment,
            stack_frame.stack_pointer,
            fault_err
        );
        kill_user_process(crate::ipc::signal::SIGSEGV);
    }

    panic!(
        "UNHANDLED EXCEPTION: PAGE FAULT (Fault Address: {:#x}, Error Code: {:#x} [{:?}])\n{}",
        fault_virt.as_u64(),
        error_code,
        fault_code,
        stack_frame
    );
}

/// Double Fault (#DF) handler — always a kernel panic.
pub extern "C" fn handle_double_fault(stack_frame: &mut InterruptStackFrame, error_code: u64) {
    panic!(
        "CPU EXCEPTION: DOUBLE FAULT (#DF, Error Code: {:#x})\n{}",
        error_code, stack_frame
    );
}

/// General Protection Fault (#GP) handler.
pub extern "C" fn handle_general_protection(
    stack_frame: &mut InterruptStackFrame,
    error_code: u64,
) {
    if (stack_frame.code_segment & 3) == 3 {
        log::warn!(
            "User process general protection fault (#GP, Code {:#x}) at RIP {:#x}",
            error_code,
            stack_frame.instruction_pointer
        );
        kill_user_process(crate::ipc::signal::SIGSEGV);
    }
    panic!(
        "CPU EXCEPTION: GENERAL PROTECTION FAULT (#GP, Error Code: {:#x})\n{}",
        error_code, stack_frame
    );
}
