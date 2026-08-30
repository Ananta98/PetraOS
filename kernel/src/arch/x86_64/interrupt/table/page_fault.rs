//! Page Fault (#PF) Interrupt Handler.
//!
//! Manages memory access violations, demand paging, and copy-on-write page table faults.
//! Ring 3 user faults result in process termination with `SIGSEGV`, while unhandled
//! Ring 0 kernel faults trigger a kernel panic with diagnostics.

use crate::arch::idt::InterruptStackFrame;
use crate::arch::read_cr2;
use crate::ipc::signal::SIGSEGV;
use crate::mm::{PageFaultErrorCode, VirtAddr};
use super::kill_user_process;

pub extern "x86-interrupt" fn page_fault_handler(
    stack_frame: &mut InterruptStackFrame,
    error_code: u64,
) {
    let fault_virt = VirtAddr::new(read_cr2());
    let fault_code = PageFaultErrorCode::from_bits_truncate(error_code);

    let cpu_id = unsafe { crate::arch::interrupt::lapic::get_lapic().id() };
    let current_thread = crate::sched::current_thread_on_cpu(cpu_id);

    if let Some(thread_arc) = current_thread {
        let thread = thread_arc.lock();
        if let Some(proc_arc) = thread.process.upgrade() {
            let proc = proc_arc.lock();
            let mut addr_space = proc.address_space.lock();
            if addr_space.handle_page_fault(fault_virt, fault_code).is_ok() {
                return;
            }
        }
    }

    if (stack_frame.code_segment & 3) == 3 || fault_virt.as_u64() <= crate::syscalls::USER_SPACE_MAX_ADDR {
        log::warn!(
            "User process page fault (SIGSEGV) at {:#x}, Error Code: {:#x} [{:?}], RIP={:#x}, CS={:#x}, RSP={:#x}",
            fault_virt.as_u64(),
            error_code,
            fault_code,
            stack_frame.instruction_pointer,
            stack_frame.code_segment,
            stack_frame.stack_pointer
        );
        kill_user_process(SIGSEGV);
    }

    panic!(
        "UNHANDLED EXCEPTION: PAGE FAULT (Fault Address: {:#x}, Error Code: {:#x} [{:?}])\n{}",
        fault_virt.as_u64(),
        error_code,
        fault_code,
        stack_frame
    );
}
