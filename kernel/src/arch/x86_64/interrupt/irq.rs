//! Interrupt Descriptor Table (IDT) Initialization.
//!
//! Embeds the GAS assembly trampolines from `irq.S` via `global_asm!` and
//! registers each stub's symbol into the IDT. All entry stubs are defined in
//! `irq.S`; Rust logic lives in `handler.rs` (dispatchers) and `fault.rs`.

use core::arch::global_asm;

use crate::arch::idt::InterruptDescriptorTable;
use crate::arch::interrupt::handler::{KEYBOARD_VECTOR, MOUSE_VECTOR};
use crate::arch::lapic_timer;

// Embed the GAS assembly file containing all exception and IRQ trampolines.
global_asm!(include_str!("irq.S"));

// ── Symbol declarations for stubs defined in irq.S ───────────────────────────
unsafe extern "C" {
    // CPU exception stubs
    fn stub_ex0();
    fn stub_ex1();
    fn stub_ex2();
    fn stub_ex3();
    fn stub_ex4();
    fn stub_ex5();
    fn stub_ex6();
    fn stub_ex7();
    fn stub_ex8();   // #DF — uses IST1
    fn stub_ex9();
    fn stub_ex10();
    fn stub_ex11();
    fn stub_ex12();
    fn stub_ex13();
    fn stub_ex14();
    fn stub_ex16();
    fn stub_ex17();
    fn stub_ex18();
    fn stub_ex19();
    fn stub_ex20();
    fn stub_ex21();

    // Hardware IRQ stubs
    fn stub_irq_33();   // PS/2 Keyboard
    fn stub_irq_44();   // PS/2 Mouse
    fn stub_irq_48();   // LAPIC Timer
    fn stub_irq_255();  // Spurious
}

static mut IDT: InterruptDescriptorTable = InterruptDescriptorTable::new();

/// Load the shared IDT on the calling CPU.
///
/// # Safety
/// Must only be called after [`init`] has populated the IDT entries.
pub unsafe fn load_idt() {
    // SAFETY: IDT is fully initialized before this is called.
    let idt_ref = unsafe { &*core::ptr::addr_of!(IDT) };
    idt_ref.load();
}

/// Initialize and load the IDT with all exception and IRQ stubs from `irq.S`.
pub fn init() {
    unsafe {
        // ── CPU Exception vectors ──────────────────────────────────────────
        IDT.entries[0].set_handler_fn(stub_ex0 as *const () as u64);
        IDT.entries[1].set_handler_fn(stub_ex1 as *const () as u64);
        IDT.entries[2].set_handler_fn(stub_ex2 as *const () as u64);
        IDT.entries[3].set_handler_fn(stub_ex3 as *const () as u64);
        IDT.entries[4].set_handler_fn(stub_ex4 as *const () as u64);
        IDT.entries[5].set_handler_fn(stub_ex5 as *const () as u64);
        IDT.entries[6].set_handler_fn(stub_ex6 as *const () as u64);
        IDT.entries[7].set_handler_fn(stub_ex7 as *const () as u64);
        IDT.entries[8].set_handler_fn(stub_ex8 as *const () as u64);
        // SAFETY: IST index 1 points to the double-fault emergency stack in TSS.
        IDT.entries[8].set_ist_index(1);
        IDT.entries[9].set_handler_fn(stub_ex9 as *const () as u64);
        IDT.entries[10].set_handler_fn(stub_ex10 as *const () as u64);
        IDT.entries[11].set_handler_fn(stub_ex11 as *const () as u64);
        IDT.entries[12].set_handler_fn(stub_ex12 as *const () as u64);
        IDT.entries[13].set_handler_fn(stub_ex13 as *const () as u64);
        IDT.entries[14].set_handler_fn(stub_ex14 as *const () as u64);
        IDT.entries[16].set_handler_fn(stub_ex16 as *const () as u64);
        IDT.entries[17].set_handler_fn(stub_ex17 as *const () as u64);
        IDT.entries[18].set_handler_fn(stub_ex18 as *const () as u64);
        IDT.entries[19].set_handler_fn(stub_ex19 as *const () as u64);
        IDT.entries[20].set_handler_fn(stub_ex20 as *const () as u64);
        IDT.entries[21].set_handler_fn(stub_ex21 as *const () as u64);

        // ── Hardware IRQ vectors ───────────────────────────────────────────
        IDT.entries[lapic_timer::TIMER_VECTOR as usize]
            .set_handler_fn(stub_irq_48 as *const () as u64);
        IDT.entries[KEYBOARD_VECTOR as usize]
            .set_handler_fn(stub_irq_33 as *const () as u64);
        IDT.entries[MOUSE_VECTOR as usize]
            .set_handler_fn(stub_irq_44 as *const () as u64);
        IDT.entries[0xFF].set_handler_fn(stub_irq_255 as *const () as u64);

        let idt_ref = &*core::ptr::addr_of!(IDT);
        idt_ref.load();
    }
}
