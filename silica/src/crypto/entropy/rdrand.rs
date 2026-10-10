// SPDX-License-Identifier: GPL-2.0

//! Hardware entropy harvester using x86_64 CPU instructions or platform fallback.

/// Attempts to read a 64-bit random value from hardware entropy (RDRAND) if available via OSTD.
///
/// Returns `None` if hardware random instructions are not supported by the CPU or hypervisor.
pub fn get_hardware_random_u64() -> Option<u64> {
    #[cfg(target_arch = "x86_64")]
    {
        ostd::arch::read_random()
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        None
    }
}
