use alloc::boxed::Box;
use super::tss::{self, CPU_TSS_POINTERS, TSS};
use x86_64::instructions::segmentation::{CS, DS, ES, FS, GS, SS, Segment};
use x86_64::instructions::tables::load_tss;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::{PrivilegeLevel, VirtAddr};

pub const KERNEL_CODE_SELECTOR: SegmentSelector = SegmentSelector::new(1, PrivilegeLevel::Ring0);
pub const KERNEL_DATA_SELECTOR: SegmentSelector = SegmentSelector::new(2, PrivilegeLevel::Ring0);
pub const USER_CODE_SELECTOR: SegmentSelector = SegmentSelector::new(3, PrivilegeLevel::Ring3);
pub const USER_DATA_SELECTOR: SegmentSelector = SegmentSelector::new(4, PrivilegeLevel::Ring3);
pub const TSS_SELECTOR: SegmentSelector = SegmentSelector::new(5, PrivilegeLevel::Ring0);

pub struct Selectors {
    pub code_selector: SegmentSelector,
    pub data_selector: SegmentSelector,
    pub user_code_selector: SegmentSelector,
    pub user_data_selector: SegmentSelector,
    pub tss_selector: SegmentSelector,
}

static mut GDT: (GlobalDescriptorTable, Option<Selectors>) = (GlobalDescriptorTable::new(), None);

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
            user_code_selector,
            user_data_selector,
            tss_selector,
        });

        gdt.load();

        CS::set_reg(code_selector);
        DS::set_reg(data_selector);
        ES::set_reg(data_selector);
        FS::set_reg(data_selector);
        GS::set_reg(data_selector);
        SS::set_reg(data_selector);
        load_tss(tss_selector);
    }
}

/// Initialises a fresh GDT and TSS for the calling CPU (used by APs).
///
/// Allocates heap-backed GDT and TSS so each AP has an independent copy.
/// The allocated memory is intentionally leaked — each CPU lives for the
/// duration of the kernel, so the memory is never freed.
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

        CS::set_reg(code_selector);
        DS::set_reg(data_selector);
        ES::set_reg(data_selector);
        FS::set_reg(data_selector);
        GS::set_reg(data_selector);
        SS::set_reg(data_selector);
        load_tss(tss_selector);

        tss_ptr as u64
    }
}
