pub mod control;
pub mod fork;
pub mod gdt;
pub mod msr;
pub mod ports;
pub mod random;
pub mod smp;
pub mod tss;
pub mod userspace;

pub use control::{
    active_address_space_root, read_cr0, read_cr2, read_cr4, read_frame_pointer,
    set_address_space_root, write_cr0, write_cr4,
};
use core::arch::asm;

/// Enable FPU and SSE/SSE2 instructions for user and kernel space.
///
/// Clears CR0.EM, sets CR0.MP, CR0.NE, clears CR0.TS, sets CR4.OSFXSR and CR4.OSXMMEXCPT,
/// and executes `fninit` to set a clean initial floating point state.
pub unsafe fn enable_sse() {
    unsafe {
        let mut cr0 = read_cr0();
        // Clear EM (bit 2), set MP (bit 1) and NE (bit 5), clear TS (bit 3)
        cr0 &= !(1 << 2); // EM
        cr0 |= (1 << 1) | (1 << 5); // MP | NE
        cr0 &= !(1 << 3); // TS
        write_cr0(cr0);

        let mut cr4 = read_cr4();
        // Set OSFXSR (bit 9) and OSXMMEXCPT (bit 10)
        cr4 |= (1 << 9) | (1 << 10);
        write_cr4(cr4);

        // SAFETY: Initialize FPU state.
        asm!("fninit", options(nomem, nostack, preserves_flags));
    }
}

/// Enable and configure the fast system call (SYSCALL / SYSRET) MSRs for a specific CPU core.
pub unsafe fn enable_syscall_for_cpu(cpu_id: usize) {
    unsafe {
        // 1. Enable System Call Extensions (SCE) in IA32_EFER (bit 0)
        let efer = msr::rdmsr(msr::IA32_EFER);
        msr::wrmsr(msr::IA32_EFER, efer | 1);

        // 2. Program IA32_STAR:
        // Bits 47:32 = Kernel CS (0x08) -> Syscall sets CS=0x08, SS=0x10.
        // Bits 63:48 = User CS/SS base selector (0x10 | 3).
        let star_val = ((0x10u64 | 3) << 48) | (0x08u64 << 32);
        msr::wrmsr(msr::IA32_STAR, star_val);

        // 3. Program IA32_LSTAR: Target RIP for syscall instruction
        unsafe extern "C" {
            fn syscall_entry();
        }

        let lstar = syscall_entry as *const () as u64;
        msr::wrmsr(msr::IA32_LSTAR, lstar);

        // 4. Program IA32_FMASK: Mask RFLAGS bits (clear IF, DF, TF, IOPL, NT, AC)
        msr::wrmsr(msr::IA32_FMASK, 0x3F7FD5);

        // 5. Program IA32_KERNEL_GS_BASE: Point to this CPU's CpuLocal
        if cpu_id < tss::MAX_CPUS {
            let locals = core::ptr::addr_of_mut!(tss::CPU_LOCALS);
            let cpu_local_ptr = core::ptr::addr_of_mut!((*locals)[cpu_id]) as u64;
            msr::wrmsr(msr::IA32_KERNEL_GS_BASE, cpu_local_ptr);
        }
    }
}

/// Enable and configure the fast system call (SYSCALL / SYSRET) MSRs on the calling CPU.
pub unsafe fn enable_syscall() {
    let cpu_id = crate::arch::cpu_id() as usize;
    // SAFETY: Delegate to enable_syscall_for_cpu with current CPU ID.
    unsafe {
        enable_syscall_for_cpu(cpu_id);
    }
}

/// Bootstrap CPU initialization: GDT/TSS, SSE, and SYSCALL MSRs.
pub fn init() {
    gdt::init();

    // SAFETY: Initializing SSE and SYSCALL MSRs for the BSP.
    unsafe {
        enable_sse();
        enable_syscall();
    }
}
