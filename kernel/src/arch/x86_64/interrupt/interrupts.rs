//! Interrupt Descriptor Table (IDT) Initialization and Management.
//!
//! Registers architectural CPU exception handlers and hardware IRQ vectors into the IDT.

use crate::arch::idt::InterruptDescriptorTable;
use crate::arch::lapic_timer;

pub use super::table::ps2::KEYBOARD_VECTOR;

static mut IDT: InterruptDescriptorTable = InterruptDescriptorTable::new();

/// Generates an assembly interrupt stub for handlers without error code.
/// Saves all general purpose registers, switches GS if needed, aligns stack,
/// calls the extern "C" handler passing &mut InterruptStackFrame in RDI,
/// restores all registers, and returns via iretq.
macro_rules! make_interrupt_stub {
    ($stub_name:ident, $handler:path) => {
        #[unsafe(naked)]
        unsafe extern "C" fn $stub_name() {
            core::arch::naked_asm!(
                // Check if interrupted from user space (CS RPL bits 0-1)
                "test byte ptr [rsp + 8], 3",
                "jz 1f",
                "swapgs",
                "1:",
                "push rax",
                "push rbx",
                "push rcx",
                "push rdx",
                "push rbp",
                "push rdi",
                "push rsi",
                "push r8",
                "push r9",
                "push r10",
                "push r11",
                "push r12",
                "push r13",
                "push r14",
                "push r15",
                "cld",
                // Pass pointer to InterruptStackFrame in RDI (15 pushes = 120 bytes)
                "lea rdi, [rsp + 120]",
                // Align stack to 16 bytes for System V ABI call
                "sub rsp, 8",
                "call {handler}",
                "add rsp, 8",
                "pop r15",
                "pop r14",
                "pop r13",
                "pop r12",
                "pop r11",
                "pop r10",
                "pop r9",
                "pop r8",
                "pop rsi",
                "pop rdi",
                "pop rbp",
                "pop rdx",
                "pop rcx",
                "pop rbx",
                "pop rax",
                // Restore user GS if returning to user space
                "test byte ptr [rsp + 8], 3",
                "jz 2f",
                "swapgs",
                "2:",
                "iretq",
                handler = sym $handler,
            );
        }
    };
}

/// Generates an assembly interrupt stub for CPU exceptions with error code.
/// Error code is already pushed by CPU. Passes &mut InterruptStackFrame in RDI,
/// and error_code in RSI.
macro_rules! make_interrupt_stub_err {
    ($stub_name:ident, $handler:path) => {
        #[unsafe(naked)]
        unsafe extern "C" fn $stub_name() {
            core::arch::naked_asm!(
                // Check if interrupted from user space (CS RPL bits 0-1 at [rsp + 16])
                "test byte ptr [rsp + 16], 3",
                "jz 1f",
                "swapgs",
                "1:",
                "push rax",
                "push rbx",
                "push rcx",
                "push rdx",
                "push rbp",
                "push rdi",
                "push rsi",
                "push r8",
                "push r9",
                "push r10",
                "push r11",
                "push r12",
                "push r13",
                "push r14",
                "push r15",
                "cld",
                // First arg (RDI): pointer to InterruptStackFrame
                // 15 registers (120 bytes) + 1 error code (8 bytes) = 128 bytes
                "lea rdi, [rsp + 128]",
                // Second arg (RSI): error_code
                "mov rsi, [rsp + 120]",
                // Stack is already 16-byte aligned for call: (48 + 120) % 16 = 8
                "call {handler}",
                "pop r15",
                "pop r14",
                "pop r13",
                "pop r12",
                "pop r11",
                "pop r10",
                "pop r9",
                "pop r8",
                "pop rsi",
                "pop rdi",
                "pop rbp",
                "pop rdx",
                "pop rcx",
                "pop rbx",
                "pop rax",
                // Pop CPU error code from stack
                "add rsp, 8",
                // Restore user GS if returning to user space
                "test byte ptr [rsp + 8], 3",
                "jz 2f",
                "swapgs",
                "2:",
                "iretq",
                handler = sym $handler,
            );
        }
    };
}

// Stubs for CPU exceptions (0..=7)
make_interrupt_stub!(stub_divide_by_zero, super::table::exceptions::divide_by_zero_handler);
make_interrupt_stub!(stub_debug, super::table::exceptions::debug_handler);
make_interrupt_stub!(stub_nmi, super::table::exceptions::nmi_handler);
make_interrupt_stub!(stub_breakpoint, super::table::exceptions::breakpoint_handler);
make_interrupt_stub!(stub_overflow, super::table::exceptions::overflow_handler);
make_interrupt_stub!(stub_bound_range, super::table::exceptions::bound_range_handler);
make_interrupt_stub!(stub_invalid_opcode, super::table::exceptions::invalid_opcode_handler);
make_interrupt_stub!(stub_device_not_available, super::table::exceptions::device_not_available_handler);

// Stubs for CPU exceptions with error code
make_interrupt_stub_err!(stub_double_fault, super::table::exceptions::double_fault_handler);
make_interrupt_stub_err!(stub_invalid_tss, super::table::exceptions::invalid_tss_handler);
make_interrupt_stub_err!(stub_segment_not_present, super::table::exceptions::segment_not_present_handler);
make_interrupt_stub_err!(stub_stack_segment_fault, super::table::exceptions::stack_segment_fault_handler);
make_interrupt_stub_err!(stub_general_protection_fault, super::table::exceptions::general_protection_fault_handler);
make_interrupt_stub_err!(stub_page_fault, super::table::page_fault::page_fault_handler);

// Architecture exceptions (16..=21)
make_interrupt_stub!(stub_x87_floating_point, super::table::exceptions::x87_floating_point_handler);
make_interrupt_stub_err!(stub_alignment_check, super::table::exceptions::alignment_check_handler);
make_interrupt_stub!(stub_machine_check, super::table::exceptions::machine_check_handler);
make_interrupt_stub!(stub_simd_floating_point, super::table::exceptions::simd_floating_point_handler);
make_interrupt_stub!(stub_virtualization_exception, super::table::exceptions::virtualization_exception_handler);
make_interrupt_stub_err!(stub_control_protection, super::table::exceptions::control_protection_handler);

// Hardware IRQs
make_interrupt_stub!(stub_timer, super::table::timer::timer_handler);
make_interrupt_stub!(stub_keyboard, super::table::ps2::keyboard_handler);
make_interrupt_stub!(stub_spurious, super::table::timer::spurious_interrupt_handler);

/// Declarative macro to register handler functions into IDT entry slots.
macro_rules! register_idt_entries {
    ($idt:expr, { $( $vector:expr => $handler:expr $(, ist: $ist:expr)? );* $(;)? }) => {
        $(
            $idt.entries[$vector as usize].set_handler_fn($handler as *const () as u64);
            $( $idt.entries[$vector as usize].set_ist_index($ist); )?
        )*
    };
}

/// Load the shared IDT on the calling CPU.
///
/// # Safety
/// Must only be called after `init()` has configured the IDT entries.
pub(crate) unsafe fn load_idt() {
    let idt_ref = unsafe { &*core::ptr::addr_of!(IDT) };
    idt_ref.load();
}

/// Initialize and load the IDT with all kernel exception and hardware interrupt handlers.
pub fn init() {
    unsafe {
        register_idt_entries!(IDT, {
            // CPU Exception Handlers (0..=7)
            0  => stub_divide_by_zero;
            1  => stub_debug;
            2  => stub_nmi;
            3  => stub_breakpoint;
            4  => stub_overflow;
            5  => stub_bound_range;
            6  => stub_invalid_opcode;
            7  => stub_device_not_available;

            // Double Fault (#DF) with dedicated IST1 stack (index 1)
            8  => stub_double_fault, ist: 1;

            // CPU Faults with error codes (10..=13)
            10 => stub_invalid_tss;
            11 => stub_segment_not_present;
            12 => stub_stack_segment_fault;
            13 => stub_general_protection_fault;

            // Page Fault (#PF)
            14 => stub_page_fault;

            // Floating point & architecture exceptions (16..=21)
            16 => stub_x87_floating_point;
            17 => stub_alignment_check;
            18 => stub_machine_check;
            19 => stub_simd_floating_point;
            20 => stub_virtualization_exception;
            21 => stub_control_protection;

            // LAPIC Timer Interrupt (vector 48)
            lapic_timer::TIMER_VECTOR => stub_timer;

            // PS/2 Keyboard Interrupt (vector 33, ISA IRQ 1)
            KEYBOARD_VECTOR => stub_keyboard;

            // Spurious Interrupt (vector 0xFF)
            0xFF => stub_spurious;
        });

        let idt_ref = &*core::ptr::addr_of!(IDT);
        idt_ref.load();
    }
}
