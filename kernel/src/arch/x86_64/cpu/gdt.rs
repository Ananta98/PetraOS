//! Global Descriptor Table (GDT) Management for x86_64 Architecture.
//!
//! Provides native GDT construction, 64-bit segment descriptors, TSS system segment descriptor,
//! and segment register reloads.

use alloc::boxed::Box;
use core::arch::asm;
use super::tss::{self, CPU_TSS_POINTERS, TaskStateSegment, TSS};
use crate::mm::VirtAddr;

/// A segment selector identifying a GDT descriptor entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct SegmentSelector(pub u16);

impl SegmentSelector {
    pub const fn new(index: u16, rpl: u8) -> Self {
        Self((index << 3) | (rpl as u16 & 0x3))
    }

    pub const fn index(self) -> u16 {
        self.0 >> 3
    }

    pub const fn rpl(self) -> u8 {
        (self.0 & 0x3) as u8
    }
}

pub const KERNEL_CODE_SELECTOR: SegmentSelector = SegmentSelector::new(1, 0);
pub const KERNEL_DATA_SELECTOR: SegmentSelector = SegmentSelector::new(2, 0);
pub const USER_CODE_SELECTOR: SegmentSelector = SegmentSelector::new(3, 3);
pub const USER_DATA_SELECTOR: SegmentSelector = SegmentSelector::new(4, 3);
pub const TSS_SELECTOR: SegmentSelector = SegmentSelector::new(5, 0);

/// Descriptor types supported in 64-bit GDT.
pub enum Descriptor {
    UserSegment(u64),
    SystemSegment(u64, u64),
}

impl Descriptor {
    pub const fn kernel_code_segment() -> Self {
        // 64-bit Code Segment: Present, Ring 0, Executable, Readable, Long mode
        Self::UserSegment(0x0020_9A00_0000_0000)
    }

    pub const fn kernel_data_segment() -> Self {
        // 64-bit Data Segment: Present, Ring 0, Writable
        Self::UserSegment(0x0000_9200_0000_0000)
    }

    pub const fn user_code_segment() -> Self {
        // 64-bit Code Segment: Present, Ring 3, Executable, Readable, Long mode
        Self::UserSegment(0x0020_FA00_0000_0000)
    }

    pub const fn user_data_segment() -> Self {
        // 64-bit Data Segment: Present, Ring 3, Writable
        Self::UserSegment(0x0000_F200_0000_0000)
    }

    pub fn tss_segment(tss: &TaskStateSegment) -> Self {
        let base = tss as *const _ as u64;
        let limit = (core::mem::size_of::<TaskStateSegment>() - 1) as u64;

        let low = (limit & 0xFFFF)
            | ((base & 0xFFFF) << 16)
            | (((base >> 16) & 0xFF) << 32)
            | (0x89 << 40) // Present, DPL 0, Type 9 (64-bit TSS available)
            | (((limit >> 16) & 0x0F) << 48)
            | (((base >> 24) & 0xFF) << 56);

        let high = base >> 32;

        Self::SystemSegment(low, high)
    }
}

#[repr(C, packed)]
struct GdtPointer {
    limit: u16,
    base: u64,
}

/// Global Descriptor Table holding up to 8 descriptors.
#[repr(C, align(16))]
pub struct GlobalDescriptorTable {
    entries: [u64; 8],
    len: usize,
}

impl GlobalDescriptorTable {
    pub const fn new() -> Self {
        Self {
            entries: [0; 8],
            len: 1, // Entry 0 is always the null descriptor
        }
    }

    pub fn append(&mut self, descriptor: Descriptor) -> SegmentSelector {
        let index = self.len;
        match descriptor {
            Descriptor::UserSegment(val) => {
                assert!(index < 8, "GDT table full");
                self.entries[index] = val;
                self.len += 1;
                SegmentSelector::new(index as u16, 0)
            }
            Descriptor::SystemSegment(low, high) => {
                assert!(index + 1 < 8, "GDT table full for system descriptor");
                self.entries[index] = low;
                self.entries[index + 1] = high;
                self.len += 2;
                SegmentSelector::new(index as u16, 0)
            }
        }
    }

    pub unsafe fn load(&self) {
        let ptr = GdtPointer {
            limit: ((self.len * core::mem::size_of::<u64>()) - 1) as u16,
            base: self.entries.as_ptr() as u64,
        };

        // SAFETY: Load GDT register.
        unsafe {
            asm!("lgdt [{}]", in(reg) &ptr, options(nomem, nostack, preserves_flags));
        }
    }
}

pub struct Selectors {
    pub code_selector: SegmentSelector,
    pub data_selector: SegmentSelector,
    pub user_code_selector: SegmentSelector,
    pub user_data_selector: SegmentSelector,
    pub tss_selector: SegmentSelector,
}

static mut GDT: (GlobalDescriptorTable, Option<Selectors>) = (GlobalDescriptorTable::new(), None);

/// Sets the Task Register (TR) to point to the TSS descriptor in the GDT.
#[inline(always)]
pub unsafe fn load_tss(selector: SegmentSelector) {
    // SAFETY: Loading Task Register.
    unsafe {
        asm!("ltr {0:x}", in(reg) selector.0, options(nomem, nostack, preserves_flags));
    }
}

/// Sets the Code Segment (CS) register via far return.
#[inline(always)]
pub unsafe fn set_cs(selector: SegmentSelector) {
    // SAFETY: Reload CS via push selector, push address, retfq.
    unsafe {
        asm!(
            "push {sel}",
            "lea {tmp}, [2f + rip]",
            "push {tmp}",
            "retfq",
            "2:",
            sel = in(reg) selector.0 as u64,
            tmp = out(reg) _,
            options(nomem, preserves_flags)
        );
    }
}

/// Sets DS, ES, FS, GS, SS data segment registers.
#[inline(always)]
pub unsafe fn set_data_segments(selector: SegmentSelector) {
    // SAFETY: Reload data segment registers.
    unsafe {
        asm!(
            "mov ds, {0:x}",
            "mov es, {0:x}",
            "mov fs, {0:x}",
            "mov gs, {0:x}",
            "mov ss, {0:x}",
            in(reg) selector.0,
            options(nomem, nostack, preserves_flags)
        );
    }
}

/// Initializes the GDT and reloads segment registers and the task register (TSS) for the BSP.
pub fn init() {
    unsafe {
        // Initialize TSS structure
        tss::init();

        let tss_ptr = core::ptr::addr_of!(TSS);
        let base = tss_ptr as u64;
        CPU_TSS_POINTERS[0] = base;

        let gdt = &mut (*core::ptr::addr_of_mut!(GDT)).0;
        let code_selector = gdt.append(Descriptor::kernel_code_segment());
        let data_selector = gdt.append(Descriptor::kernel_data_segment());
        let user_code_selector = gdt.append(Descriptor::user_code_segment());
        let user_data_selector = gdt.append(Descriptor::user_data_segment());
        let tss_ref = &*tss_ptr;
        let tss_selector = gdt.append(Descriptor::tss_segment(tss_ref));

        (*core::ptr::addr_of_mut!(GDT)).1 = Some(Selectors {
            code_selector,
            data_selector,
            user_code_selector: SegmentSelector::new(user_code_selector.index(), 3),
            user_data_selector: SegmentSelector::new(user_data_selector.index(), 3),
            tss_selector,
        });

        gdt.load();

        set_cs(code_selector);
        set_data_segments(data_selector);
        load_tss(tss_selector);
    }
}

/// Initialises a fresh GDT and TSS for the calling CPU (used by APs).
pub fn init_per_cpu() -> u64 {
    unsafe {
        // Allocate a separate double-fault stack for this AP (IST1).
        const STACK_SIZE: usize = 4096 * 5;
        let df_stack: Box<[u8; STACK_SIZE]> = Box::new([0u8; STACK_SIZE]);
        let df_stack_ptr = Box::into_raw(df_stack);
        let df_stack_end = df_stack_ptr as u64 + STACK_SIZE as u64;

        // Allocate a separate exception stack for this AP (IST2).
        let exc_stack: Box<[u8; STACK_SIZE]> = Box::new([0u8; STACK_SIZE]);
        let exc_stack_ptr = Box::into_raw(exc_stack);
        let exc_stack_end = exc_stack_ptr as u64 + STACK_SIZE as u64;

        // Allocate and initialise TSS.
        let mut tss = Box::new(TaskStateSegment::new());
        tss.interrupt_stack_table[0] = VirtAddr::new(df_stack_end);
        tss.interrupt_stack_table[1] = VirtAddr::new(exc_stack_end);
        let tss_ptr = Box::into_raw(tss);

        // Allocate and build GDT with this AP's TSS.
        let mut gdt = Box::new(GlobalDescriptorTable::new());
        let code_selector = gdt.append(Descriptor::kernel_code_segment());
        let data_selector = gdt.append(Descriptor::kernel_data_segment());
        let _user_code_selector = gdt.append(Descriptor::user_code_segment());
        let _user_data_selector = gdt.append(Descriptor::user_data_segment());
        let tss_selector = gdt.append(Descriptor::tss_segment(&*tss_ptr));

        let gdt_ptr = Box::into_raw(gdt);
        (*gdt_ptr).load();

        set_cs(code_selector);
        set_data_segments(data_selector);
        load_tss(tss_selector);

        tss_ptr as u64
    }
}
