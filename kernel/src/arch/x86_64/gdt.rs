use core::arch::asm;

/// A GDT Segment Descriptor.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct SegmentDescriptor {
    low: u32,
    high: u32,
}

impl SegmentDescriptor {
    /// Creates a new, empty (null) segment descriptor.
    pub const fn null() -> Self {
        SegmentDescriptor { low: 0, high: 0 }
    }

    /// Creates a 64-bit Kernel Code Segment descriptor.
    pub const fn kernel_code() -> Self {
        let high = (1 << 11) // Executable
            | (1 << 12)      // Descriptor Type (code/data)
            | (1 << 15)      // Present
            | (1 << 21);     // 64-bit (L) flag
        SegmentDescriptor { low: 0, high }
    }

    /// Creates a 64-bit Kernel Data Segment descriptor.
    pub const fn kernel_data() -> Self {
        let high = (1 << 9)  // Writable
            | (1 << 12)      // Descriptor Type (code/data)
            | (1 << 15);     // Present
        SegmentDescriptor { low: 0, high }
    }

    /// Creates a 64-bit User Code Segment descriptor.
    pub const fn user_code() -> Self {
        let high = (1 << 11) // Executable
            | (1 << 12)      // Descriptor Type (code/data)
            | (3 << 13)      // DPL = 3
            | (1 << 15)      // Present
            | (1 << 21);     // 64-bit (L) flag
        SegmentDescriptor { low: 0, high }
    }

    /// Creates a 64-bit User Data Segment descriptor.
    pub const fn user_data() -> Self {
        let high = (1 << 9)  // Writable
            | (1 << 12)      // Descriptor Type (code/data)
            | (3 << 13)      // DPL = 3
            | (1 << 15);     // Present
        SegmentDescriptor { low: 0, high }
    }

    /// Creates a TSS descriptor from a base address and limit.
    /// Since a TSS descriptor is 16 bytes in x86_64, this returns two SegmentDescriptors.
    pub fn tss(base: u64, limit: u32) -> (Self, Self) {
        let limit_low = limit & 0xFFFF;
        let limit_high = (limit >> 16) & 0xF;
        
        let base_low = (base & 0xFFFF) as u32;
        let base_mid = ((base >> 16) & 0xFF) as u32;
        let base_high = ((base >> 24) & 0xFF) as u32;
        
        let low = (base_low << 16) | limit_low;
        
        let type_field = 0x9; // 64-bit TSS (Available)
        let present = 1 << 15;
        let high = base_high << 24
            | (limit_high << 16)
            | present
            | (type_field << 8)
            | base_mid;
            
        let desc_low = SegmentDescriptor { low, high };
        
        let base_upper = (base >> 32) as u32;
        let desc_high = SegmentDescriptor {
            low: base_upper,
            high: 0,
        };
        
        (desc_low, desc_high)
    }
}

/// The structure passed to `lgdt`.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
struct GdtPointer {
    limit: u16,
    base: u64,
}

/// Global Descriptor Table.
#[derive(Debug, Clone)]
#[repr(C, align(8))]
pub struct GlobalDescriptorTable {
    entries: [SegmentDescriptor; 7],
}

impl GlobalDescriptorTable {
    pub const fn new() -> Self {
        GlobalDescriptorTable {
            entries: [
                SegmentDescriptor::null(),
                SegmentDescriptor::kernel_code(),
                SegmentDescriptor::kernel_data(),
                SegmentDescriptor::user_code(),
                SegmentDescriptor::user_data(),
                SegmentDescriptor::null(),
                SegmentDescriptor::null(),
            ],
        }
    }

    /// Loads the GDT into the CPU.
    pub fn load(&self) {
        let ptr = GdtPointer {
            limit: (core::mem::size_of::<GlobalDescriptorTable>() - 1) as u16,
            base: self as *const _ as u64,
        };

        unsafe {
            asm!("lgdt [{}]", in(reg) &ptr, options(nostack, preserves_flags));
        }
    }
}

static mut GDT: GlobalDescriptorTable = GlobalDescriptorTable::new();

/// Initializes the GDT and reloads segment registers and the task register (TSS).
pub fn init() {
    unsafe {
        // Initialize TSS structure
        super::tss::init();
        
        // Get TSS base and limit
        let tss_ptr = core::ptr::addr_of!(super::tss::TSS);
        let base = tss_ptr as u64;
        let limit = (core::mem::size_of::<super::tss::TaskStateSegment>() - 1) as u32;
        
        // Build and write TSS descriptors
        let (tss_low, tss_high) = SegmentDescriptor::tss(base, limit);
        let gdt_mut = &mut *core::ptr::addr_of_mut!(GDT);
        gdt_mut.entries[5] = tss_low;
        gdt_mut.entries[6] = tss_high;
        
        gdt_mut.load();
        
        // Reload segment registers and load the TSS selector (0x28).
        asm!(
            "push 0x08",
            "lea rax, [rip + 2f]",
            "push rax",
            "retfq",
            "2:",
            "mov ax, 0x10",
            "mov ds, ax",
            "mov es, ax",
            "mov fs, ax",
            "mov gs, ax",
            "mov ss, ax",
            "mov ax, 0x28",
            "ltr ax",
            out("rax") _,
            options(preserves_flags)
        );
    }
}
