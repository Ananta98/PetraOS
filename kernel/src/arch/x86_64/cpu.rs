//! CPU management, feature identification, segment register control, and CPU information queries.

use core::str;
use ostd::arch::cpu::context::{FsBase, GsBase};
use ostd::arch::cpu::cpuid::cpuid;
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CpuFeatures {
    /// Indicates support for FSGSBASE instructions (`rdfsbase`, `wrfsbase`).
    pub has_fsgsbase: bool,
    /// Indicates support for No-Execute / Execute-Disable page protection bit.
    pub has_nxe: bool,
    /// Indicates support for the Time Stamp Counter (`rdtsc`).
    pub has_tsc: bool,
    /// Indicates support for local APIC timer TSC deadline mode.
    pub has_tsc_deadline: bool,
    /// Indicates support for APIC on-chip.
    pub has_apic: bool,
    /// Indicates support for x2APIC.
    pub has_x2apic: bool,
    /// Indicates support for SSE extensions.
    pub has_sse: bool,
    /// Indicates support for SSE2 extensions.
    pub has_sse2: bool,
    /// Indicates support for SSE3 extensions.
    pub has_sse3: bool,
    /// Indicates support for SSSE3 extensions.
    pub has_ssse3: bool,
    /// Indicates support for SSE4.1 extensions.
    pub has_sse4_1: bool,
    /// Indicates support for SSE4.2 extensions.
    pub has_sse4_2: bool,
    /// Indicates support for AVX instruction extensions.
    pub has_avx: bool,
    /// Indicates support for AVX2 instruction extensions.
    pub has_avx2: bool,
    /// Indicates support for AVX512 Foundation instruction extensions.
    pub has_avx512f: bool,
    /// Indicates support for FMA (Fused Multiply-Add) extensions.
    pub has_fma: bool,
    /// Indicates support for RDRAND instruction.
    pub has_rdrand: bool,
    /// Indicates support for RDSEED instruction.
    pub has_rdseed: bool,
    /// Indicates support for XSAVE/XRSTOR processor extended states.
    pub has_xsave: bool,
    /// Indicates support for 1GB huge pages (PDPE1GB).
    pub has_1gb_pages: bool,
    /// Indicates that the CPU is executing inside a hypervisor / virtual machine.
    pub has_hypervisor: bool,
}

impl CpuFeatures {
    /// Detects supported CPU hardware features dynamically via CPUID instruction.
    pub fn get() -> Self {
        let mut features = Self {
            has_fsgsbase: true,
            has_nxe: true,
            has_tsc: true,
            ..Default::default()
        };

        // CPUID Leaf 1: Feature Identifiers
        if let Some(res) = cpuid(1, 0) {
            features.has_tsc = (res.edx & (1 << 4)) != 0;
            features.has_apic = (res.edx & (1 << 9)) != 0;
            features.has_sse = (res.edx & (1 << 25)) != 0;
            features.has_sse2 = (res.edx & (1 << 26)) != 0;

            features.has_sse3 = (res.ecx & (1 << 0)) != 0;
            features.has_ssse3 = (res.ecx & (1 << 9)) != 0;
            features.has_fma = (res.ecx & (1 << 12)) != 0;
            features.has_sse4_1 = (res.ecx & (1 << 19)) != 0;
            features.has_sse4_2 = (res.ecx & (1 << 20)) != 0;
            features.has_x2apic = (res.ecx & (1 << 21)) != 0;
            features.has_tsc_deadline = (res.ecx & (1 << 24)) != 0;
            features.has_xsave = (res.ecx & (1 << 26)) != 0;
            features.has_avx = (res.ecx & (1 << 28)) != 0;
            features.has_rdrand = (res.ecx & (1 << 30)) != 0;
            features.has_hypervisor = (res.ecx & (1 << 31)) != 0;
        }

        // CPUID Leaf 7 (subleaf 0): Extended Feature Flags
        if let Some(res) = cpuid(7, 0) {
            features.has_fsgsbase = (res.ebx & (1 << 0)) != 0 || features.has_fsgsbase;
            features.has_avx2 = (res.ebx & (1 << 5)) != 0;
            features.has_avx512f = (res.ebx & (1 << 16)) != 0;
            features.has_rdseed = (res.ebx & (1 << 18)) != 0;
        }

        // CPUID Leaf 0x80000001: Extended Processor Info and Feature Bits
        if let Some(res) = cpuid(0x8000_0001, 0) {
            features.has_nxe = (res.edx & (1 << 20)) != 0 || features.has_nxe;
            features.has_1gb_pages = (res.edx & (1 << 26)) != 0;
        }

        features
    }
}

/// Comprehensive hardware information about the CPU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CpuInfo {
    /// Vendor identification string bytes (12 bytes, e.g. "GenuineIntel" or "AuthenticAMD").
    pub vendor_id: [u8; 12],
    /// Brand/Model string bytes (48 bytes).
    pub brand_string: [u8; 48],
    /// Effective CPU family.
    pub family: u32,
    /// Effective CPU model.
    pub model: u32,
    /// Stepping revision identifier.
    pub stepping: u32,
    /// Processor type.
    pub processor_type: u32,
    /// Physical address width in bits (e.g., 36, 48, 52).
    pub physical_address_bits: u8,
    /// Virtual address width in bits (e.g., 48, 57).
    pub virtual_address_bits: u8,
    /// Cache line size in bytes (e.g., 64).
    pub cache_line_size: u32,
    /// Total number of logical CPUs available in the system.
    pub num_logical_cpus: usize,
    /// Detected hardware features and capabilities.
    pub features: CpuFeatures,
}

impl CpuInfo {
    /// Queries the CPU hardware via CPUID and returns complete CPU information.
    pub fn get() -> Self {
        let mut vendor_id = [0u8; 12];
        let mut brand_string = [0u8; 48];
        let mut family = 0;
        let mut model = 0;
        let mut stepping = 0;
        let mut processor_type = 0;
        let mut physical_address_bits = 36;
        let mut virtual_address_bits = 48;
        let mut cache_line_size = 64;

        // Leaf 0x00: Vendor ID
        if let Some(res) = cpuid(0, 0) {
            let ebx = res.ebx.to_ne_bytes();
            let edx = res.edx.to_ne_bytes();
            let ecx = res.ecx.to_ne_bytes();
            vendor_id[0..4].copy_from_slice(&ebx);
            vendor_id[4..8].copy_from_slice(&edx);
            vendor_id[8..12].copy_from_slice(&ecx);
        }

        // Leaf 0x01: Signature (Family, Model, Stepping) and Cache line
        if let Some(res) = cpuid(1, 0) {
            stepping = res.eax & 0xF;
            let base_model = (res.eax >> 4) & 0xF;
            let base_family = (res.eax >> 8) & 0xF;
            processor_type = (res.eax >> 12) & 0x3;
            let ext_model = (res.eax >> 16) & 0xF;
            let ext_family = (res.eax >> 20) & 0xFF;

            family = if base_family == 15 {
                base_family + ext_family
            } else {
                base_family
            };

            model = if base_family == 6 || base_family == 15 {
                (ext_model << 4) | base_model
            } else {
                base_model
            };

            let cl_flush = (res.ebx >> 8) & 0xFF;
            if cl_flush > 0 {
                cache_line_size = cl_flush * 8;
            }
        }

        // Extended Leaves 0x80000002..0x80000004: Brand String
        let mut brand_offset = 0;
        for leaf in [0x8000_0002u32, 0x8000_0003u32, 0x8000_0004u32] {
            if let Some(res) = cpuid(leaf, 0) {
                if brand_offset + 16 <= brand_string.len() {
                    brand_string[brand_offset..brand_offset + 4]
                        .copy_from_slice(&res.eax.to_ne_bytes());
                    brand_string[brand_offset + 4..brand_offset + 8]
                        .copy_from_slice(&res.ebx.to_ne_bytes());
                    brand_string[brand_offset + 8..brand_offset + 12]
                        .copy_from_slice(&res.ecx.to_ne_bytes());
                    brand_string[brand_offset + 12..brand_offset + 16]
                        .copy_from_slice(&res.edx.to_ne_bytes());
                    brand_offset += 16;
                }
            }
        }

        // Extended Leaf 0x80000008: Address Sizes
        if let Some(res) = cpuid(0x8000_0008, 0) {
            let phys = (res.eax & 0xFF) as u8;
            let virt = ((res.eax >> 8) & 0xFF) as u8;
            if phys > 0 {
                physical_address_bits = phys;
            }
            if virt > 0 {
                virtual_address_bits = virt;
            }
        }

        Self {
            vendor_id,
            brand_string,
            family,
            model,
            stepping,
            processor_type,
            physical_address_bits,
            virtual_address_bits,
            cache_line_size,
            num_logical_cpus: num_cpus(),
            features: CpuFeatures::get(),
        }
    }

    /// Returns the CPU vendor identification string (e.g., "GenuineIntel" or "AuthenticAMD").
    pub fn vendor(&self) -> &str {
        let s = str::from_utf8(&self.vendor_id)
            .map(|s| s.trim_matches('\0').trim())
            .unwrap_or("");
        if s.is_empty() { "Unknown" } else { s }
    }

    /// Returns the CPU brand name string (e.g., "QEMU Virtual CPU...").
    pub fn brand(&self) -> &str {
        let s = str::from_utf8(&self.brand_string)
            .map(|s| s.trim_matches('\0').trim())
            .unwrap_or("");
        if s.is_empty() {
            "Generic x86_64 CPU"
        } else {
            s
        }
    }

    /// Checks if the CPU is manufactured by Intel.
    pub fn is_intel(&self) -> bool {
        self.vendor() == "GenuineIntel"
    }

    /// Checks if the CPU is manufactured by AMD.
    pub fn is_amd(&self) -> bool {
        self.vendor() == "AuthenticAMD"
    }

    /// Checks if the processor is running inside a hypervisor / virtual machine environment.
    pub fn is_virtualized(&self) -> bool {
        self.features.has_hypervisor
    }
}

/// Query and return the complete CPU information structure.
pub fn get_cpu_info() -> CpuInfo {
    CpuInfo::get()
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

    #[ktest]
    fn test_cpu_info() {
        let info = CpuInfo::get();
        assert!(!info.vendor().is_empty());
        assert!(!info.brand().is_empty());
        assert!(info.physical_address_bits >= 32);
        assert!(info.virtual_address_bits >= 32);
        assert!(info.cache_line_size > 0);
        assert!(info.num_logical_cpus > 0);
        assert_eq!(info, get_cpu_info());
    }
}
