//! Central Interrupt and Exception Dispatcher.
//!
//! Replaces the bloated per-handler approach with a metadata-table-driven generic
//! handler for standard exceptions, and dedicated fast paths for page fault,
//! double fault, and general protection fault.
//!
//! IRQ routing dispatches hardware vectors (timer, keyboard, spurious) to their
//! respective handler functions.

use crate::arch::idt::InterruptStackFrame;
use crate::arch::ports::Ports;
use crate::ipc::signal::{SIGBUS, SIGFPE, SIGILL, SIGSEGV, SIGTRAP};

/// Metadata for a standard CPU exception used by the generic handler.
struct ExceptionMeta {
    /// Short mnemonic, e.g. "#DE".
    mnemonic: &'static str,
    /// POSIX signal to deliver to the user process (0 = always panic).
    user_signal: u8,
    /// Whether the exception has a CPU-pushed error code.
    has_error_code: bool,
}

/// Table of metadata for all 32 Intel-defined CPU exceptions (vectors 0–21).
/// Vectors 22–31 are reserved and handled by the fallback panic path.
static EXCEPTION_META: [ExceptionMeta; 22] = [
    ExceptionMeta {
        mnemonic: "#DE Divide Error",
        user_signal: SIGFPE,
        has_error_code: false,
    }, // 0
    ExceptionMeta {
        mnemonic: "#DB Debug",
        user_signal: SIGTRAP,
        has_error_code: false,
    }, // 1
    ExceptionMeta {
        mnemonic: "#NMI Non-Maskable Interrupt",
        user_signal: 0,
        has_error_code: false,
    }, // 2
    ExceptionMeta {
        mnemonic: "#BP Breakpoint",
        user_signal: SIGTRAP,
        has_error_code: false,
    }, // 3
    ExceptionMeta {
        mnemonic: "#OF Overflow",
        user_signal: SIGFPE,
        has_error_code: false,
    }, // 4
    ExceptionMeta {
        mnemonic: "#BR Bound Range Exceeded",
        user_signal: SIGSEGV,
        has_error_code: false,
    }, // 5
    ExceptionMeta {
        mnemonic: "#UD Invalid Opcode",
        user_signal: SIGILL,
        has_error_code: false,
    }, // 6
    ExceptionMeta {
        mnemonic: "#NM Device Not Available",
        user_signal: 0,
        has_error_code: false,
    }, // 7
    ExceptionMeta {
        mnemonic: "#DF Double Fault",
        user_signal: 0,
        has_error_code: true,
    }, // 8
    ExceptionMeta {
        mnemonic: "#MF Coprocessor Overrun",
        user_signal: SIGFPE,
        has_error_code: false,
    }, // 9
    ExceptionMeta {
        mnemonic: "#TS Invalid TSS",
        user_signal: 0,
        has_error_code: true,
    }, // 10
    ExceptionMeta {
        mnemonic: "#NP Segment Not Present",
        user_signal: 0,
        has_error_code: true,
    }, // 11
    ExceptionMeta {
        mnemonic: "#SS Stack Segment Fault",
        user_signal: 0,
        has_error_code: true,
    }, // 12
    ExceptionMeta {
        mnemonic: "#GP General Protection",
        user_signal: SIGSEGV,
        has_error_code: true,
    }, // 13
    ExceptionMeta {
        mnemonic: "#PF Page Fault",
        user_signal: SIGSEGV,
        has_error_code: true,
    }, // 14
    ExceptionMeta {
        mnemonic: "Reserved (15)",
        user_signal: 0,
        has_error_code: false,
    }, // 15
    ExceptionMeta {
        mnemonic: "#MF x87 FPU Error",
        user_signal: SIGFPE,
        has_error_code: false,
    }, // 16
    ExceptionMeta {
        mnemonic: "#AC Alignment Check",
        user_signal: SIGBUS,
        has_error_code: true,
    }, // 17
    ExceptionMeta {
        mnemonic: "#MC Machine Check",
        user_signal: 0,
        has_error_code: false,
    }, // 18
    ExceptionMeta {
        mnemonic: "#XM SIMD FPU Exception",
        user_signal: SIGFPE,
        has_error_code: false,
    }, // 19
    ExceptionMeta {
        mnemonic: "#VE Virtualization Exception",
        user_signal: 0,
        has_error_code: false,
    }, // 20
    ExceptionMeta {
        mnemonic: "#CP Control Protection",
        user_signal: 0,
        has_error_code: true,
    }, // 21
];

/// Central exception dispatcher called from the interrupt stubs in `interrupts.rs`.
///
/// Routes vectors with dedicated handlers (8, 13, 14) to their fast-path functions.
/// All other standard exceptions go through the generic metadata-table handler.
///
/// # Safety
/// Called from naked assembly stubs; `frame` must point to a valid interrupt stack frame.
#[unsafe(no_mangle)]
pub extern "C" fn handle_exception(
    stack_frame: &mut InterruptStackFrame,
    error_code: u64,
    vector: u64,
) {
    match vector {
        14 => super::fault::handle_page_fault(stack_frame, error_code),
        8 => super::fault::handle_double_fault(stack_frame, error_code),
        13 => super::fault::handle_general_protection(stack_frame, error_code),
        _ => handle_generic_exception(stack_frame, error_code, vector),
    }
}

/// Generic handler for all non-specialized CPU exceptions.
fn handle_generic_exception(stack_frame: &mut InterruptStackFrame, error_code: u64, vector: u64) {
    let meta = EXCEPTION_META.get(vector as usize);
    let mnemonic = meta.map(|m| m.mnemonic).unwrap_or("Unknown Exception");
    let user_signal = meta.map(|m| m.user_signal).unwrap_or(0);
    let is_user = (stack_frame.code_segment & 3) == 3;

    // Non-maskable interrupt and machine check always panic regardless of privilege.
    if vector == 2 || vector == 18 {
        panic!("CPU EXCEPTION: {}\n{}", mnemonic, stack_frame);
    }

    // Debug and breakpoint are informational when triggered from kernel.
    if (vector == 1 || vector == 3) && !is_user {
        log::warn!("CPU EXCEPTION: {}\n{}", mnemonic, stack_frame);
        return;
    }

    if is_user && user_signal != 0 {
        log::warn!(
            "User process {} at RIP {:#x}",
            mnemonic,
            stack_frame.instruction_pointer
        );
        super::fault::kill_user_process(user_signal);
    }

    if meta.map(|m| m.has_error_code).unwrap_or(false) {
        panic!(
            "CPU EXCEPTION: {} (Error Code: {:#x})\n{}",
            mnemonic, error_code, stack_frame
        );
    } else {
        panic!("CPU EXCEPTION: {}\n{}", mnemonic, stack_frame);
    }
}

/// Interrupt vector assigned to the PS/2 Keyboard (ISA IRQ 1).
pub const KEYBOARD_VECTOR: u8 = 33;

/// Interrupt vector assigned to the PS/2 Mouse (ISA IRQ 12).
pub const MOUSE_VECTOR: u8 = 44;

/// Central IRQ dispatcher called from the interrupt stubs in `interrupts.rs`.
///
/// # Safety
/// Called from naked assembly stubs; `frame` must point to a valid interrupt stack frame.
#[unsafe(no_mangle)]
pub extern "C" fn handle_irq(stack_frame: &mut InterruptStackFrame, vector: u64) {
    match vector as u8 {
        crate::arch::lapic_timer::TIMER_VECTOR => super::timer::handle_timer(stack_frame),
        KEYBOARD_VECTOR => handle_keyboard(),
        MOUSE_VECTOR => handle_mouse(),
        0xFF => super::timer::handle_spurious(stack_frame),
        _ => {
            log::warn!("Unhandled IRQ vector {}", vector);
        }
    }
}

/// PS/2 Keyboard interrupt handler.
fn handle_keyboard() {
    // Drain pending bytes from 8042 controller output buffer.
    loop {
        // SAFETY: Reading status port 0x64 has no side effects.
        let status = unsafe { Ports::inb(0x64) };
        if (status & 0x01) == 0 {
            break;
        }
        // SAFETY: Reading data port 0x60 clears the 8042 output buffer.
        let byte = unsafe { Ports::inb(0x60) };
        // Route keyboard data (bit 5 clear) vs mouse data (bit 5 set).
        if (status & 0x20) == 0 {
            crate::drivers::char::keyboard::handle_scancode(byte);
        } else {
            crate::drivers::char::mouse::handle_mouse_byte(byte);
        }
    }
    // SAFETY: LAPIC is guaranteed to be initialized when receiving interrupts.
    unsafe {
        crate::arch::interrupt::lapic::get_lapic().end_of_interrupt();
    }
}

/// PS/2 Mouse interrupt handler (ISA IRQ 12).
fn handle_mouse() {
    // Drain pending bytes from 8042 controller output buffer.
    loop {
        // SAFETY: Reading status port 0x64 has no side effects.
        let status = unsafe { Ports::inb(0x64) };
        if (status & 0x01) == 0 {
            break;
        }
        // SAFETY: Reading data port 0x60 clears the 8042 output buffer.
        let byte = unsafe { Ports::inb(0x60) };
        // Route mouse data (bit 5 set) vs keyboard data (bit 5 clear).
        if (status & 0x20) != 0 {
            crate::drivers::char::mouse::handle_mouse_byte(byte);
        } else {
            crate::drivers::char::keyboard::handle_scancode(byte);
        }
    }
    // SAFETY: LAPIC is guaranteed to be initialized when receiving interrupts.
    unsafe {
        crate::arch::interrupt::lapic::get_lapic().end_of_interrupt();
    }
}
