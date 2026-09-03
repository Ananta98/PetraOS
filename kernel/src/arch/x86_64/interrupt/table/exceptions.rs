//! x86_64 CPU Architectural Exception Handlers.
//!
//! Handles synchronous processor exceptions. For Ring 3 (user-mode) faults,
//! the offending process is terminated with the corresponding POSIX signal without
//! crashing the kernel. For Ring 0 (kernel-mode) faults, the handler routes to
//! `panic!()` to trigger complete diagnostics and backtrace generation.

use crate::arch::idt::InterruptStackFrame;
use super::kill_user_process;

pub extern "C" fn divide_by_zero_handler(stack_frame: &mut InterruptStackFrame) {
    if (stack_frame.code_segment & 3) == 3 {
        log::warn!(
            "User process divide by zero (#DE) at RIP {:#x}",
            stack_frame.instruction_pointer
        );
        kill_user_process(crate::ipc::signal::SIGFPE);
    }
    panic!("CPU EXCEPTION: DIVIDE BY ZERO (#DE)\n{}", stack_frame);
}

pub extern "C" fn debug_handler(stack_frame: &mut InterruptStackFrame) {
    log::warn!("EXCEPTION: DEBUG (#DB)\n{}", stack_frame);
}

pub extern "C" fn nmi_handler(stack_frame: &mut InterruptStackFrame) {
    panic!("EXCEPTION: NON-MASKABLE INTERRUPT (#NMI)\n{}", stack_frame);
}

pub extern "C" fn breakpoint_handler(stack_frame: &mut InterruptStackFrame) {
    log::warn!("EXCEPTION: BREAKPOINT (#BP)\n{}", stack_frame);
}

pub extern "C" fn overflow_handler(stack_frame: &mut InterruptStackFrame) {
    if (stack_frame.code_segment & 3) == 3 {
        log::warn!(
            "User process overflow (#OF) at RIP {:#x}",
            stack_frame.instruction_pointer
        );
        kill_user_process(crate::ipc::signal::SIGFPE);
    }
    panic!("CPU EXCEPTION: OVERFLOW (#OF)\n{}", stack_frame);
}

pub extern "C" fn bound_range_handler(stack_frame: &mut InterruptStackFrame) {
    if (stack_frame.code_segment & 3) == 3 {
        log::warn!(
            "User process bound range exceeded (#BR) at RIP {:#x}",
            stack_frame.instruction_pointer
        );
        kill_user_process(crate::ipc::signal::SIGSEGV);
    }
    panic!("CPU EXCEPTION: BOUND RANGE EXCEEDED (#BR)\n{}", stack_frame);
}

pub extern "C" fn invalid_opcode_handler(stack_frame: &mut InterruptStackFrame) {
    if (stack_frame.code_segment & 3) == 3 {
        log::warn!(
            "User process invalid opcode (#UD) at RIP {:#x}",
            stack_frame.instruction_pointer
        );
        kill_user_process(crate::ipc::signal::SIGILL);
    }
    panic!("CPU EXCEPTION: INVALID OPCODE (#UD)\n{}", stack_frame);
}

pub extern "C" fn device_not_available_handler(stack_frame: &mut InterruptStackFrame) {
    panic!("CPU EXCEPTION: DEVICE NOT AVAILABLE (#NM)\n{}", stack_frame);
}

pub extern "C" fn double_fault_handler(
    stack_frame: &mut InterruptStackFrame,
    error_code: u64,
) {
    panic!(
        "CPU EXCEPTION: DOUBLE FAULT (#DF, Error Code: {:#x})\n{}",
        error_code,
        stack_frame
    );
}

pub extern "C" fn invalid_tss_handler(
    stack_frame: &mut InterruptStackFrame,
    error_code: u64,
) {
    panic!(
        "CPU EXCEPTION: INVALID TSS (#TS, Error Code: {:#x})\n{}",
        error_code,
        stack_frame
    );
}

pub extern "C" fn segment_not_present_handler(
    stack_frame: &mut InterruptStackFrame,
    error_code: u64,
) {
    panic!(
        "CPU EXCEPTION: SEGMENT NOT PRESENT (#NP, Error Code: {:#x})\n{}",
        error_code,
        stack_frame
    );
}

pub extern "C" fn stack_segment_fault_handler(
    stack_frame: &mut InterruptStackFrame,
    error_code: u64,
) {
    panic!(
        "CPU EXCEPTION: STACK SEGMENT FAULT (#SS, Error Code: {:#x})\n{}",
        error_code,
        stack_frame
    );
}

pub extern "C" fn general_protection_fault_handler(
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
        error_code,
        stack_frame
    );
}

pub extern "C" fn x87_floating_point_handler(stack_frame: &mut InterruptStackFrame) {
    if (stack_frame.code_segment & 3) == 3 {
        log::warn!(
            "User process x87 FPU error (#MF) at RIP {:#x}",
            stack_frame.instruction_pointer
        );
        kill_user_process(crate::ipc::signal::SIGFPE);
    }
    panic!("CPU EXCEPTION: x87 FPU FLOATING POINT ERROR (#MF)\n{}", stack_frame);
}

pub extern "C" fn alignment_check_handler(
    stack_frame: &mut InterruptStackFrame,
    error_code: u64,
) {
    if (stack_frame.code_segment & 3) == 3 {
        log::warn!(
            "User process alignment check (#AC, Code {:#x}) at RIP {:#x}",
            error_code,
            stack_frame.instruction_pointer
        );
        kill_user_process(crate::ipc::signal::SIGBUS);
    }
    panic!(
        "CPU EXCEPTION: ALIGNMENT CHECK (#AC, Error Code: {:#x})\n{}",
        error_code,
        stack_frame
    );
}

pub extern "C" fn machine_check_handler(stack_frame: &mut InterruptStackFrame) {
    panic!("CPU EXCEPTION: MACHINE CHECK (#MC)\n{}", stack_frame);
}

pub extern "C" fn simd_floating_point_handler(stack_frame: &mut InterruptStackFrame) {
    if (stack_frame.code_segment & 3) == 3 {
        log::warn!(
            "User process SIMD exception (#XM) at RIP {:#x}",
            stack_frame.instruction_pointer
        );
        kill_user_process(crate::ipc::signal::SIGFPE);
    }
    panic!(
        "CPU EXCEPTION: SIMD FLOATING POINT EXCEPTION (#XM)\n{}",
        stack_frame
    );
}

pub extern "C" fn virtualization_exception_handler(
    stack_frame: &mut InterruptStackFrame,
) {
    panic!("CPU EXCEPTION: VIRTUALIZATION EXCEPTION (#VE)\n{}", stack_frame);
}

pub extern "C" fn control_protection_handler(
    stack_frame: &mut InterruptStackFrame,
    error_code: u64,
) {
    panic!(
        "CPU EXCEPTION: CONTROL PROTECTION EXCEPTION (#CP, Error Code: {:#x})\n{}",
        error_code,
        stack_frame
    );
}
