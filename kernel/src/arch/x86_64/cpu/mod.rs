pub mod context;
pub mod gdt;
pub mod msr;
pub mod ports;
pub mod smp;
pub mod stack;
pub mod tss;
pub mod userspace;

use core::arch::asm;

/// Enable FPU and SSE/SSE2 instructions for user and kernel space.
///
/// Clears CR0.EM, sets CR0.MP, CR0.NE, clears CR0.TS, sets CR4.OSFXSR and CR4.OSXMMEXCPT,
/// and executes `fninit` to set a clean initial floating point state.
pub unsafe fn enable_sse() {
    let mut cr0: u64;
    let mut cr4: u64;

    // SAFETY: Read and write CR0 to configure FPU/SSE control flags.
    unsafe {
        asm!("mov {}, cr0", out(reg) cr0, options(nomem, nostack, preserves_flags));
        cr0 &= !(1 << 2); // Clear CR0.EM (Emulation)
        cr0 |= 1 << 1;    // Set CR0.MP (Monitor Coprocessor)
        cr0 |= 1 << 5;    // Set CR0.NE (Numeric Error)
        cr0 &= !(1 << 3); // Clear CR0.TS (Task Switched)
        asm!("mov cr0, {}", in(reg) cr0, options(nomem, nostack, preserves_flags));

        // SAFETY: Read and write CR4 to enable OSFXSR and OSXMMEXCPT.
        asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack, preserves_flags));
        cr4 |= 1 << 9;    // Set CR4.OSFXSR (Operating System Support for FXSAVE and FXRSTOR)
        cr4 |= 1 << 10;   // Set CR4.OSXMMEXCPT (Operating System Support for SIMD Floating-Point Exceptions)
        asm!("mov cr4, {}", in(reg) cr4, options(nomem, nostack, preserves_flags));

        // SAFETY: Initialize FPU state.
        asm!("fninit", options(nomem, nostack, preserves_flags));
    }
}

/// Enable and configure the fast system call (SYSCALL / SYSRET) MSRs on the calling CPU.
pub unsafe fn enable_syscall() {
    // SAFETY: IA32 MSRs configuration for enabling x86_64 fast syscall handling.
    unsafe {
        // 1. Enable System Call Extensions (SCE) in IA32_EFER
        let mut efer = msr::rdmsr(msr::IA32_EFER);
        efer |= 1; // Bit 0: SCE
        msr::wrmsr(msr::IA32_EFER, efer);

        // 2. Program IA32_STAR:
        // Bits 47:32 = Kernel CS (0x08) -> Syscall sets CS=0x08, SS=0x10.
        // Bits 63:48 = User CS/SS base selector (0x10 | 3).
        let star = ((0x10u64 | 3) << 48) | (0x08u64 << 32);
        msr::wrmsr(msr::IA32_STAR, star);

        // 3. Program IA32_LSTAR: Target RIP for syscall instruction
        unsafe extern "C" {
            fn syscall_fast_entry();
        }
        let lstar = syscall_fast_entry as *const () as u64;
        msr::wrmsr(msr::IA32_LSTAR, lstar);

        // 4. Program IA32_FMASK: Mask RFLAGS bits (clear IF, DF, TF, IOPL, NT, AC)
        let fmask = 0x3F7FD5u64;
        msr::wrmsr(msr::IA32_FMASK, fmask);

        // 5. Program IA32_KERNEL_GS_BASE: Point to this CPU's CpuLocal
        let cpu_id = crate::arch::cpu_id() as usize;
        if cpu_id < tss::MAX_CPUS {
            let locals = core::ptr::addr_of_mut!(tss::CPU_LOCALS);
            let cpu_local_ptr = core::ptr::addr_of_mut!((*locals)[cpu_id]) as u64;
            msr::wrmsr(msr::IA32_KERNEL_GS_BASE, cpu_local_ptr);
        }
    }
}

pub fn init() {
    gdt::init();

    // SAFETY: Initializing SSE and SYSCALL MSRs for the BSP.
    unsafe {
        enable_sse();
        enable_syscall();
    }
}


