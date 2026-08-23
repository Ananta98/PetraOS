//! Model-Specific Registers (MSRs) helper functions for x86_64.
//!
//! Implements native rdmsr, wrmsr, and FS/GS base register manipulation.

use core::arch::asm;

pub const IA32_EFER: u32 = 0xC000_0080;
pub const IA32_STAR: u32 = 0xC000_0081;
pub const IA32_LSTAR: u32 = 0xC000_0082;
pub const IA32_FMASK: u32 = 0xC000_0084;
pub const IA32_FS_BASE: u32 = 0xC000_0100;
pub const IA32_GS_BASE: u32 = 0xC000_0101;
pub const IA32_KERNEL_GS_BASE: u32 = 0xC000_0102;

/// Read a 64-bit value from an MSR.
///
/// # Safety
/// The caller must ensure that `msr` is a valid MSR register on the CPU.
#[inline(always)]
pub unsafe fn rdmsr(msr: u32) -> u64 {
    let low: u32;
    let high: u32;
    // SAFETY: rdmsr reads from MSR index in ECX into EDX:EAX.
    unsafe {
        asm!(
            "rdmsr",
            in("ecx") msr,
            out("eax") low,
            out("edx") high,
            options(nomem, nostack, preserves_flags)
        );
    }
    ((high as u64) << 32) | (low as u64)
}

/// Write a 64-bit value to an MSR.
///
/// # Safety
/// The caller must ensure that `msr` is a valid MSR register on the CPU and `val` contains valid bits.
#[inline(always)]
pub unsafe fn wrmsr(msr: u32, val: u64) {
    let low = val as u32;
    let high = (val >> 32) as u32;
    // SAFETY: wrmsr writes EDX:EAX to MSR index in ECX.
    unsafe {
        asm!(
            "wrmsr",
            in("ecx") msr,
            in("eax") low,
            in("edx") high,
            options(nomem, nostack, preserves_flags)
        );
    }
}

/// Read the current FS base address.
#[inline(always)]
pub fn read_fs_base() -> u64 {
    unsafe { rdmsr(IA32_FS_BASE) }
}

/// Write the current FS base address.
#[inline(always)]
pub fn write_fs_base(base: u64) {
    unsafe { wrmsr(IA32_FS_BASE, base) }
}

/// Read the current GS base address.
#[inline(always)]
pub fn read_gs_base() -> u64 {
    unsafe { rdmsr(IA32_GS_BASE) }
}

/// Write the current GS base address.
#[inline(always)]
pub fn write_gs_base(base: u64) {
    unsafe { wrmsr(IA32_GS_BASE, base) }
}
