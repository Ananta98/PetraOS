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

pub static mut TSS: TaskStateSegment = TaskStateSegment::new();

pub static mut CPU_TSS_POINTERS: [u64; 8] = [0; 8];

pub fn init() {
    unsafe {
        let df_stack_start = core::ptr::addr_of!(DOUBLE_FAULT_STACK) as u64;
        let df_stack_end = df_stack_start + STACK_SIZE as u64;
        // IST1 is index 0
        TSS.ist[0] = df_stack_end;

        let exc_stack_start = core::ptr::addr_of!(EXCEPTION_STACK) as u64;
        let exc_stack_end = exc_stack_start + STACK_SIZE as u64;
        // IST2 is index 1
        TSS.ist[1] = exc_stack_end;
    }
}


/// Set the Privilege Level 0 Kernel Stack Pointer (RSP0) in the TSS.
///
/// Used during Ring 3 execution so interrupts and system calls can switch to a valid kernel stack.
pub fn set_rsp0(rsp: u64) {
    unsafe {
        TSS.rsp[0] = rsp;
    }
}

