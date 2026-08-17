pub mod context;
pub mod gdt;
pub mod msr;
pub mod ports;
pub mod rdtsc;
pub mod smp;
pub mod stack;
pub mod tss;
pub mod userspace;

use x86_64::registers::control::{Cr0, Cr0Flags, Cr2, Cr3, Cr3Flags, Cr4, Cr4Flags};
use x86_64::registers::model_specific::{Efer, EferFlags, KernelGsBase, LStar, SFMask, Star};
use x86_64::registers::rflags::RFlags;
use x86_64::structures::paging::PhysFrame;
use x86_64::{PhysAddr, VirtAddr};

/// Sets the active page table physical root address (CR3).
///
/// # Safety
/// The caller must ensure `root` points to a valid root page table (PML4) physical address.
#[inline(always)]
pub unsafe fn set_address_space_root(root: u64) {
    let frame = PhysFrame::containing_address(PhysAddr::new(root));
    unsafe {
        Cr3::write(frame, Cr3Flags::empty());
    }
}

/// Returns the current active page table physical root address (CR3).
#[inline(always)]
pub fn active_address_space_root() -> u64 {
    Cr3::read().0.start_address().as_u64()
}

/// Returns the linear address that caused the latest page fault (CR2).
#[inline(always)]
pub fn read_cr2() -> u64 {
    Cr2::read_raw()
}

/// Enable FPU and SSE/SSE2 instructions for user and kernel space.
///
/// Clears CR0.EM, sets CR0.MP, CR0.NE, clears CR0.TS, sets CR4.OSFXSR and CR4.OSXMMEXCPT,
/// and executes `fninit` to set a clean initial floating point state.
pub unsafe fn enable_sse() {
    // SAFETY: Read and write CR0/CR4 to configure FPU/SSE control flags.
    unsafe {
        let mut cr0 = Cr0::read();
        cr0.remove(Cr0Flags::EMULATE_COPROCESSOR);
        cr0.insert(Cr0Flags::MONITOR_COPROCESSOR | Cr0Flags::NUMERIC_ERROR);
        cr0.remove(Cr0Flags::TASK_SWITCHED);
        Cr0::write(cr0);

        let mut cr4 = Cr4::read();
        cr4.insert(Cr4Flags::OSFXSR | Cr4Flags::OSXMMEXCPT_ENABLE);
        Cr4::write(cr4);

        // SAFETY: Initialize FPU state.
        core::arch::asm!("fninit", options(nomem, nostack, preserves_flags));
    }
}

/// Enable and configure the fast system call (SYSCALL / SYSRET) MSRs on the calling CPU.
pub unsafe fn enable_syscall() {
    // SAFETY: IA32 MSRs configuration for enabling x86_64 fast syscall handling.
    unsafe {
        // 1. Enable System Call Extensions (SCE) in IA32_EFER
        let mut efer = Efer::read();
        efer.insert(EferFlags::SYSTEM_CALL_EXTENSIONS);
        Efer::write(efer);

        // 2. Program IA32_STAR:
        // Bits 47:32 = Kernel CS (0x08) -> Syscall sets CS=0x08, SS=0x10.
        // Bits 63:48 = User CS/SS base selector (0x10 | 3).
        let _ = Star::write(
            gdt::USER_CODE_SELECTOR,
            gdt::USER_DATA_SELECTOR,
            gdt::KERNEL_CODE_SELECTOR,
            gdt::KERNEL_DATA_SELECTOR,
        );

        // 3. Program IA32_LSTAR: Target RIP for syscall instruction
        unsafe extern "C" {
            fn syscall_fast_entry();
        }
        let lstar = syscall_fast_entry as *const () as u64;
        LStar::write(VirtAddr::new(lstar));

        // 4. Program IA32_FMASK: Mask RFLAGS bits (clear IF, DF, TF, IOPL, NT, AC)
        let fmask = RFlags::from_bits_truncate(0x3F7FD5);
        SFMask::write(fmask);

        // 5. Program IA32_KERNEL_GS_BASE: Point to this CPU's CpuLocal
        let cpu_id = crate::arch::cpu_id() as usize;
        if cpu_id < tss::MAX_CPUS {
            let locals = core::ptr::addr_of_mut!(tss::CPU_LOCALS);
            let cpu_local_ptr = core::ptr::addr_of_mut!((*locals)[cpu_id]) as u64;
            KernelGsBase::write(VirtAddr::new(cpu_local_ptr));
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
