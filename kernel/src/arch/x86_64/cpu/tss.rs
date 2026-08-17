use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;

const STACK_SIZE: usize = 4096 * 5; // 20 KiB stack

#[repr(align(16))]
struct Stack([u8; STACK_SIZE]);

static mut DOUBLE_FAULT_STACK: Stack = Stack([0; STACK_SIZE]);
static mut EXCEPTION_STACK: Stack = Stack([0; STACK_SIZE]);

pub const MAX_CPUS: usize = 8;

pub static mut TSS: TaskStateSegment = TaskStateSegment::new();

pub static mut CPU_TSS_POINTERS: [u64; MAX_CPUS] = [0; MAX_CPUS];

/// Per-CPU scratch data used during fast system calls (e.g., SYSCALL instruction)
#[repr(C, align(16))]
#[derive(Debug, Clone, Copy)]
pub struct CpuLocal {
    pub kernel_rsp: u64,
    pub user_rsp_scratch: u64,
}

impl CpuLocal {
    pub const fn new() -> Self {
        Self {
            kernel_rsp: 0,
            user_rsp_scratch: 0,
        }
    }
}

pub static mut CPU_LOCALS: [CpuLocal; MAX_CPUS] = [CpuLocal::new(); MAX_CPUS];

pub fn init() {
    unsafe {
        let df_stack_start = core::ptr::addr_of!(DOUBLE_FAULT_STACK) as u64;
        let df_stack_end = df_stack_start + STACK_SIZE as u64;
        // IST1 is index 0
        let tss_mut = &mut *core::ptr::addr_of_mut!(TSS);
        tss_mut.interrupt_stack_table[0] = VirtAddr::new(df_stack_end);

        let exc_stack_start = core::ptr::addr_of!(EXCEPTION_STACK) as u64;
        let exc_stack_end = exc_stack_start + STACK_SIZE as u64;
        // IST2 is index 1
        tss_mut.interrupt_stack_table[1] = VirtAddr::new(exc_stack_end);

        let bsp_tss_addr = core::ptr::addr_of!(TSS) as u64;
        let tss_ptrs = core::ptr::addr_of_mut!(CPU_TSS_POINTERS);
        (*tss_ptrs)[0] = bsp_tss_addr;
    }
}

/// Set the Privilege Level 0 Kernel Stack Pointer (RSP0) in the TSS and CpuLocal.
///
/// Used during Ring 3 execution so interrupts and system calls can switch to a valid kernel stack.
pub fn set_rsp0(rsp: u64) {
    unsafe {
        let cpu_id = crate::arch::cpu_id() as usize;
        if cpu_id < MAX_CPUS {
            let tss_ptrs = core::ptr::addr_of_mut!(CPU_TSS_POINTERS);
            let tss_addr = (*tss_ptrs)[cpu_id];
            if tss_addr != 0 {
                let tss_ptr = tss_addr as *mut TaskStateSegment;
                (*tss_ptr).privilege_stack_table[0] = VirtAddr::new(rsp);
            } else {
                let tss_mut = &mut *core::ptr::addr_of_mut!(TSS);
                tss_mut.privilege_stack_table[0] = VirtAddr::new(rsp);
            }

            let locals = core::ptr::addr_of_mut!(CPU_LOCALS);
            (*locals)[cpu_id].kernel_rsp = rsp;
        }
    }
}
