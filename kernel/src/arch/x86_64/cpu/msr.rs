//! Model-Specific Registers (MSRs) helper functions for x86_64.

use x86_64::registers::model_specific::{FsBase, GsBase, Msr};
use x86_64::VirtAddr;

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
    // SAFETY: Delegated to x86_64 Msr wrapper under caller's safety contract.
    unsafe { Msr::new(msr).read() }
}

/// Write a 64-bit value to an MSR.
///
/// # Safety
/// The caller must ensure that `msr` is a valid MSR register on the CPU and `val` contains valid bits.
#[inline(always)]
pub unsafe fn wrmsr(msr: u32, val: u64) {
    // SAFETY: Delegated to x86_64 Msr wrapper under caller's safety contract.
    unsafe {
        let mut m = Msr::new(msr);
        m.write(val);
    }
}

/// Read the current FS base address.
#[inline(always)]
pub fn read_fs_base() -> u64 {
    FsBase::read().as_u64()
}

/// Write the current FS base address.
#[inline(always)]
pub fn write_fs_base(base: u64) {
    FsBase::write(VirtAddr::new(base));
}

/// Read the current GS base address.
#[inline(always)]
pub fn read_gs_base() -> u64 {
    GsBase::read().as_u64()
}

/// Write the current GS base address.
#[inline(always)]
pub fn write_gs_base(base: u64) {
    GsBase::write(VirtAddr::new(base));
}
