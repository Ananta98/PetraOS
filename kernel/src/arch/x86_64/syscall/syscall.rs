use core::arch::global_asm;

global_asm!(include_str!("Syscall.S"));

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SyscallFrame {
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub rax: u64,
    // Pushed by CPU on interrupt (e.g. int 0x80)
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

impl SyscallFrame {
    pub fn syscall_num(&self) -> u64 {
        self.rax
    }

    pub fn arg1(&self) -> u64 {
        self.rdi
    }

    pub fn arg2(&self) -> u64 {
        self.rsi
    }

    pub fn arg3(&self) -> u64 {
        self.rdx
    }

    pub fn arg4(&self) -> u64 {
        if self.r10 != 0 { self.r10 } else { self.rcx }
    }

    pub fn arg5(&self) -> u64 {
        self.r8
    }

    pub fn arg6(&self) -> u64 {
        self.r9
    }

    pub fn set_return_value(&mut self, val: u64) {
        self.rax = val;
    }

    pub fn setup_user_entry(&mut self, entry_point: u64, stack_pointer: u64) {
        self.rip = entry_point;
        self.rsp = stack_pointer;
        self.cs = 0x1B; // User code selector with RPL=3
        self.ss = 0x23; // User data selector with RPL=3
        self.rflags = 0x202; // Enable interrupts (IF flag)
    }
}

/// System call entry handler invoked from `Syscall.S`.
///
/// # Safety
/// Called from assembly entry with `rdi` pointing to a valid `SyscallFrame` saved on the stack.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_syscall(frame: &mut SyscallFrame) {
    let ret = crate::syscalls::dispatch(frame);
    frame.set_return_value(ret);
}
