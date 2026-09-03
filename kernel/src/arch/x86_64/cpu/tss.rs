//! Task State Segment (TSS) and Per-CPU State for x86_64 Architecture.
//!
//! Provides the 64-bit Task State Segment structure and CPU-local structures for interrupt/syscall stack switching.

use crate::mm::VirtAddr;

/// 64-bit Task State Segment (TSS) layout specified by the Intel/AMD64 architecture.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct TaskStateSegment {
    reserved_1: u32,
    /// Privilege Stack Table: [RSP0, RSP1, RSP2]
    pub privilege_stack_table: [VirtAddr; 3],
    reserved_2: u64,
    /// Interrupt Stack Table: [IST1, IST2, IST3, IST4, IST5, IST6, IST7]
    pub interrupt_stack_table: [VirtAddr; 7],
    reserved_3: u64,
    reserved_4: u16,
    /// I/O Map Base Address (offset from TSS base). Setting >= size of TSS disables I/O bitmap.
    pub iomap_base: u16,
}

impl TaskStateSegment {
    /// Creates a zeroed Task State Segment with I/O bitmap disabled.
    pub const fn new() -> Self {
        Self {
            reserved_1: 0,
            privilege_stack_table: [VirtAddr::zero(); 3],
            reserved_2: 0,
            interrupt_stack_table: [VirtAddr::zero(); 7],
            reserved_3: 0,
            reserved_4: 0,
            iomap_base: core::mem::size_of::<Self>() as u16,
        }
    }
}

const STACK_SIZE: usize = 4096 * 5; // 20 KiB stack

#[repr(align(16))]
struct Stack([u8; STACK_SIZE]);

pub const MAX_CPUS: usize = 8;

static mut DOUBLE_FAULT_STACKS: [Stack; MAX_CPUS] = [const { Stack([0; STACK_SIZE]) }; MAX_CPUS];
static mut EXCEPTION_STACKS: [Stack; MAX_CPUS] = [const { Stack([0; STACK_SIZE]) }; MAX_CPUS];

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

/// Configure IST1 (Double Fault) and IST2 (Exception) for a specific CPU core.
pub fn init_ist_for_cpu(cpu_id: usize, tss: &mut TaskStateSegment) {
    if cpu_id < MAX_CPUS {
        unsafe {
            let df_stack_start = core::ptr::addr_of!(DOUBLE_FAULT_STACKS[cpu_id]) as u64;
            let df_stack_end = df_stack_start + STACK_SIZE as u64;
            tss.interrupt_stack_table[0] = VirtAddr::new(df_stack_end);

            let exc_stack_start = core::ptr::addr_of!(EXCEPTION_STACKS[cpu_id]) as u64;
            let exc_stack_end = exc_stack_start + STACK_SIZE as u64;
            tss.interrupt_stack_table[1] = VirtAddr::new(exc_stack_end);
        }
    }
}

pub fn init() {
    unsafe {
        let tss_mut = &mut *core::ptr::addr_of_mut!(TSS);
        init_ist_for_cpu(0, tss_mut);

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
        let target_cpu = if cpu_id < MAX_CPUS { cpu_id } else { 0 };

        let tss_ptrs = core::ptr::addr_of_mut!(CPU_TSS_POINTERS);
        let tss_addr = (*tss_ptrs)[target_cpu];
        if tss_addr != 0 {
            let tss_ptr = tss_addr as *mut TaskStateSegment;
            (*tss_ptr).privilege_stack_table[0] = VirtAddr::new(rsp);
        } else {
            let tss_mut = &mut *core::ptr::addr_of_mut!(TSS);
            tss_mut.privilege_stack_table[0] = VirtAddr::new(rsp);
        }

        let locals = core::ptr::addr_of_mut!(CPU_LOCALS);
        (*locals)[target_cpu].kernel_rsp = rsp;
    }
}
