use core::arch::asm;

/// The interrupt stack frame pushed by the CPU on exception/interrupt.
#[derive(Clone)]
#[repr(C)]
pub struct InterruptStackFrame {
    pub instruction_pointer: u64,
    pub code_segment: u64,
    pub cpu_flags: u64,
    pub stack_pointer: u64,
    pub stack_segment: u64,
}

impl core::fmt::Display for InterruptStackFrame {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "InterruptStackFrame {{\n    RIP: {:#018x},\n    CS:  {:#06x},\n    RFLAGS: {:#018x},\n    RSP: {:#018x},\n    SS:  {:#06x},\n}}",
            self.instruction_pointer,
            self.code_segment,
            self.cpu_flags,
            self.stack_pointer,
            self.stack_segment
        )
    }
}

impl core::fmt::Debug for InterruptStackFrame {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(self, f)
    }
}

pub type HandlerFunc = extern "C" fn(&mut InterruptStackFrame);
pub type HandlerFuncWithErrCode = extern "C" fn(&mut InterruptStackFrame, u64);
pub type PageFaultHandlerFunc = extern "C" fn(&mut InterruptStackFrame, u64);

#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct IdtEntry {
    offset_low: u16,
    segment_selector: u16,
    options: u16,
    offset_middle: u16,
    offset_high: u32,
    reserved: u32,
}

impl IdtEntry {
    pub const fn missing() -> Self {
        IdtEntry {
            offset_low: 0,
            segment_selector: 0,
            options: 0,
            offset_middle: 0,
            offset_high: 0,
            reserved: 0,
        }
    }

    /// Set handler function for the entry.
    /// segment_selector should be the Kernel Code Segment (0x08).
    pub fn set_handler_fn(&mut self, handler: u64) {
        self.offset_low = handler as u16;
        self.segment_selector = 0x08; // Kernel Code Segment
        self.options = 0x8E00; // Present = 1, DPL = 0, Type = Interrupt Gate
        self.offset_middle = (handler >> 16) as u16;
        self.offset_high = (handler >> 32) as u32;
        self.reserved = 0;
    }

    /// Set user-accessible handler function (DPL = 3).
    pub fn set_user_handler_fn(&mut self, handler: u64) {
        self.offset_low = handler as u16;
        self.segment_selector = 0x08; // Kernel Code Segment
        self.options = 0xEE00; // Present = 1, DPL = 3, Type = Interrupt Gate
        self.offset_middle = (handler >> 16) as u16;
        self.offset_high = (handler >> 32) as u32;
        self.reserved = 0;
    }

    /// Sets the Interrupt Stack Table (IST) index for this IDT entry (1-indexed).
    ///
    /// # Safety
    /// Caller must ensure that the index points to a valid TSS IST entry.
    pub unsafe fn set_ist_index(&mut self, index: u16) {
        self.options = (self.options & 0xFFF8) | (index & 0x7);
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
struct IdtPointer {
    limit: u16,
    base: u64,
}

#[derive(Clone)]
#[repr(C, align(16))]
pub struct InterruptDescriptorTable {
    pub entries: [IdtEntry; 256],
}

impl InterruptDescriptorTable {
    pub const fn new() -> Self {
        InterruptDescriptorTable {
            entries: [IdtEntry::missing(); 256],
        }
    }

    pub fn load(&self) {
        let ptr = IdtPointer {
            limit: (core::mem::size_of::<InterruptDescriptorTable>() - 1) as u16,
            base: self as *const _ as u64,
        };

        unsafe {
            asm!("lidt [{}]", in(reg) &ptr, options(nostack, preserves_flags));
        }
    }
}
