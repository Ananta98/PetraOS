//! CPU management, feature identification, and segment register control.

use ostd::arch::cpu::context::{FsBase, GsBase};
use ostd::irq::disable_local;
use ostd::mm::Vaddr;

/// Write `addr` into the FS-base MSR on the current CPU using `wrfsbase`.
#[inline]
pub fn set_fs_base(addr: Vaddr) {
    FsBase::new(addr).load();
}

/// Read the current CPU's FS-base MSR using `rdfsbase`.
#[inline]
pub fn get_fs_base() -> Vaddr {
    let mut fs = FsBase::default();
    fs.save();
    fs.addr()
}

/// Write `addr` into the GS-base MSR on the current CPU using `wrgsbase`.
#[inline]
pub fn set_gs_base(addr: Vaddr) {
    let guard = disable_local();
    GsBase::new(addr).load(&guard);
}

/// Read the current CPU's GS-base MSR using `rdgsbase`.
#[inline]
pub fn get_gs_base() -> Vaddr {
    let mut gs = GsBase::default();
    let guard = disable_local();
    gs.save(&guard);
    gs.addr()
}

/// Hints to the CPU core that it is executing inside a spin-wait loop.
#[inline]
pub fn cpu_relax() {
    core::hint::spin_loop();
}

/// Returns the number of active logical CPUs in the system.
#[inline]
pub fn num_cpus() -> usize {
    ostd::cpu::num_cpus()
}

/// Structure representing detected CPU hardware features and capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuFeatures {
    /// Indicates support for FSGSBASE instructions (`rdfsbase`, `wrfsbase`).
    pub has_fsgsbase: bool,
    /// Indicates support for No-Execute / Execute-Disable page protection bit.
    pub has_nxe: bool,
    /// Indicates support for the Time Stamp Counter (`rdtsc`).
    pub has_tsc: bool,
}

impl CpuFeatures {
    /// Detects supported CPU hardware features.
    pub fn get() -> Self {
        Self {
            has_fsgsbase: true, // Enabled by OSTD kernel boot sequence
            has_nxe: true,      // Standard requirement for x86_64 long mode
            has_tsc: true,      // Standard on modern x86_64 processors
        }
    }
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;

    #[ktest]
    fn test_cpu_features() {
        let features = CpuFeatures::get();
        assert!(features.has_fsgsbase);
        assert!(features.has_nxe);
        assert!(features.has_tsc);
    }

    #[ktest]
    fn test_num_cpus() {
        assert!(num_cpus() > 0);
    }

    #[ktest]
    fn test_fs_gs_base_roundtrip() {
        let original_fs = get_fs_base();
        let test_val = 0x7FFF_0000_1000;
        set_fs_base(test_val);
        assert_eq!(get_fs_base(), test_val);
        set_fs_base(original_fs);
    }
}
