const STACK_SIZE: usize = 4096 * 5; // 20 KiB stack

#[allow(dead_code)]
#[repr(align(16))]
struct Stack([u8; STACK_SIZE]);

static mut DOUBLE_FAULT_STACK: Stack = Stack([0; STACK_SIZE]);

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

pub fn init() {
    unsafe {
        let stack_start = core::ptr::addr_of!(DOUBLE_FAULT_STACK) as u64;
        let stack_end = stack_start + STACK_SIZE as u64;
        // IST1 is index 0
        TSS.ist[0] = stack_end;
    }
}
