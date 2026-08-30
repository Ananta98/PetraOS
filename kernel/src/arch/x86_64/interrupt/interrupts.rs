//! Interrupt Descriptor Table (IDT) Initialization and Management.
//!
//! Registers architectural CPU exception handlers and hardware IRQ vectors into the IDT.

use crate::arch::idt::InterruptDescriptorTable;
use crate::arch::lapic_timer;

pub use super::table::ps2::KEYBOARD_VECTOR;

static mut IDT: InterruptDescriptorTable = InterruptDescriptorTable::new();

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
            0  => super::table::exceptions::divide_by_zero_handler;
            1  => super::table::exceptions::debug_handler;
            2  => super::table::exceptions::nmi_handler;
            3  => super::table::exceptions::breakpoint_handler;
            4  => super::table::exceptions::overflow_handler;
            5  => super::table::exceptions::bound_range_handler;
            6  => super::table::exceptions::invalid_opcode_handler;
            7  => super::table::exceptions::device_not_available_handler;

            // Double Fault (#DF) with dedicated IST1 stack (index 1)
            8  => super::table::exceptions::double_fault_handler, ist: 1;

            // CPU Faults with error codes (10..=13)
            10 => super::table::exceptions::invalid_tss_handler;
            11 => super::table::exceptions::segment_not_present_handler;
            12 => super::table::exceptions::stack_segment_fault_handler;
            13 => super::table::exceptions::general_protection_fault_handler;

            // Page Fault (#PF)
            14 => super::table::page_fault::page_fault_handler;

            // Floating point & architecture exceptions (16..=21)
            16 => super::table::exceptions::x87_floating_point_handler;
            17 => super::table::exceptions::alignment_check_handler;
            18 => super::table::exceptions::machine_check_handler;
            19 => super::table::exceptions::simd_floating_point_handler;
            20 => super::table::exceptions::virtualization_exception_handler;
            21 => super::table::exceptions::control_protection_handler;

            // LAPIC Timer Interrupt (vector 48)
            lapic_timer::TIMER_VECTOR => super::table::timer::timer_handler;

            // PS/2 Keyboard Interrupt (vector 33, ISA IRQ 1)
            KEYBOARD_VECTOR => super::table::ps2::keyboard_handler;

            // Spurious Interrupt (vector 0xFF)
            0xFF => super::table::timer::spurious_interrupt_handler;
        });

        let idt_ref = &*core::ptr::addr_of!(IDT);
        idt_ref.load();
    }
}
