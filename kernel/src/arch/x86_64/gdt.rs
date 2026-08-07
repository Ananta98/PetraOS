use core::arch::asm;
use alloc::boxed::Box;

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
        super::tss::CPU_TSS_POINTERS[0] = base;
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

/// Initialises a fresh GDT and TSS for the calling CPU (used by APs).
///
/// Allocates heap-backed GDT and TSS so each AP has an independent copy.
/// The allocated memory is intentionally leaked — each CPU lives for the
/// duration of the kernel, so the memory is never freed.
pub fn init_per_cpu() -> u64 {
    unsafe {
        // Allocate a separate double-fault stack for this AP.
        const STACK_SIZE: usize = 4096 * 5;
        let df_stack: Box<[u8; STACK_SIZE]> = Box::new([0u8; STACK_SIZE]);
        let stack_ptr = Box::into_raw(df_stack);
        let stack_end = stack_ptr as u64 + STACK_SIZE as u64;

        // Allocate and initialise TSS.
        let tss = Box::new(super::tss::TaskStateSegment::new());
        let tss_ptr = Box::into_raw(tss);
        (*tss_ptr).ist[0] = stack_end;

        // Allocate and build GDT with this AP's TSS.
        let mut gdt = Box::new(GlobalDescriptorTable::new());
        let base = tss_ptr as u64;
        let limit = (core::mem::size_of::<super::tss::TaskStateSegment>() - 1) as u32;
        let (tss_low, tss_high) = SegmentDescriptor::tss(base, limit);
        gdt.entries[5] = tss_low;
        gdt.entries[6] = tss_high;

        let gdt_ptr = Box::into_raw(gdt);
        (*gdt_ptr).load();

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

        base
    }
}
