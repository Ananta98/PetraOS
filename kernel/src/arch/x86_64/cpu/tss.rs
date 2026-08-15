const STACK_SIZE: usize = 4096 * 5; // 20 KiB stack

#[allow(dead_code)]
#[repr(align(16))]
struct Stack([u8; STACK_SIZE]);

static mut DOUBLE_FAULT_STACK: Stack = Stack([0; STACK_SIZE]);
static mut EXCEPTION_STACK: Stack = Stack([0; STACK_SIZE]);

#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct TaskStateSegment {
    reserved0: u32,
    pub rsp: [u64; 3],
    reserved1: u64,
    pub ist: [u64; 7],
    reserved2: u64,
    reserved3: u16,
    pub iomap_base: u16,
}

impl TaskStateSegment {
    pub const fn new() -> Self {
        TaskStateSegment {
            reserved0: 0,
            rsp: [0; 3],
            reserved1: 0,
            ist: [0; 7],
            reserved2: 0,
            reserved3: 0,
            iomap_base: 104, // No IO map
        }
    }
}

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
        tss_mut.ist[0] = df_stack_end;

        let exc_stack_start = core::ptr::addr_of!(EXCEPTION_STACK) as u64;
        let exc_stack_end = exc_stack_start + STACK_SIZE as u64;
        // IST2 is index 1
        tss_mut.ist[1] = exc_stack_end;

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
                (*tss_ptr).rsp[0] = rsp;
            } else {
                let tss_mut = &mut *core::ptr::addr_of_mut!(TSS);
                tss_mut.rsp[0] = rsp;
            }

            let locals = core::ptr::addr_of_mut!(CPU_LOCALS);
            (*locals)[cpu_id].kernel_rsp = rsp;
        }
    }
}

