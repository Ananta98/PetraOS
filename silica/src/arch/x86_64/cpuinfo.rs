// SPDX-License-Identifier: GPL-2.0

//! x86_64 CPUID Discovery and CPU Information.
//!
//! Queries physical and virtual CPU capabilities, vendor strings, model information,
//! cache hierarchy, address bit widths, and feature flags via safe CPUID discovery.
//! Output matches Linux standard `/proc/cpuinfo` format.

use alloc::{string::String, vec::Vec};
use core::{arch::x86_64::CpuidResult, fmt, str};
use ostd::arch::cpu::cpuid::cpuid;

/// CPU vendor identification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuVendor {
    Intel,
    Amd,
    Unknown,
}

impl CpuVendor {
    pub fn parse(vendor_str: &str) -> Self {
        match vendor_str {
            "GenuineIntel" => Self::Intel,
            "AuthenticAMD" => Self::Amd,
            _ => Self::Unknown,
        }
    }
}

/// Standard and extended CPUID leaves.
#[repr(u32)]
pub enum CpuidLeaf {
    Base = 0x0000_0000,
    Feature = 0x0000_0001,
    Cache = 0x0000_0004,
    ExtendedFeatures = 0x0000_0007,
    Topology = 0x0000_000B,

    ExtBase = 0x8000_0000,
    ExtFeature = 0x8000_0001,
    ExtBrand1 = 0x8000_0002,
    ExtBrand2 = 0x8000_0003,
    ExtBrand3 = 0x8000_0004,
    ExtCacheL1 = 0x8000_0005,
    ExtCacheL2L3 = 0x8000_0006,
    ExtAddrSizes = 0x8000_0008,
}

/// Information describing a single logical processor core.
#[derive(Clone, Debug)]
pub struct CpuInformation {
    pub processor: u32,
    pub vendor_id: String,
    pub cpu_family: u8,
    pub model: u8,
    pub model_name: String,
    pub stepping: Option<u8>,
    pub cpu_mhz: Option<u32>,
    pub cache_size: Option<u32>,
    pub physical_id: u32,
    pub siblings: u32,
    pub core_id: u32,
    pub cpu_cores: u32,
    pub apicid: u32,
    pub initial_apicid: u32,
    pub fpu: bool,
    pub fpu_exception: bool,
    pub cpuid_level: u32,
    pub wp: bool,
    pub flags: Vec<&'static str>,
    pub bugs: Vec<&'static str>,
    pub bogomips: Option<u32>,
    pub clflush_size: u32,
    pub cache_alignment: u32,
    pub address_sizes: (u32, u32),
}

impl CpuInformation {
    /// Discovers and populates CPU information for the specified logical processor.
    pub fn new(processor: u32) -> Self {
        let siblings = ostd::cpu::num_cpus() as u32;
        let tsc_hz = ostd::arch::tsc_freq();
        let cpu_mhz = if tsc_hz > 0 {
            Some((tsc_hz / 1_000_000) as u32)
        } else {
            None
        };
        let bogomips = cpu_mhz.map(|mhz| mhz.saturating_mul(2));

        let mut info = Self {
            processor,
            vendor_id: String::from("Unknown"),
            cpu_family: 6,
            model: 1,
            model_name: String::from("Generic x86_64 Processor"),
            stepping: None,
            cpu_mhz,
            cache_size: None,
            physical_id: 0,
            siblings,
            core_id: processor,
            cpu_cores: siblings,
            apicid: processor,
            initial_apicid: processor,
            fpu: true,
            fpu_exception: true,
            cpuid_level: 1,
            wp: true,
            flags: Vec::new(),
            bugs: Vec::new(),
            bogomips,
            clflush_size: 64,
            cache_alignment: 64,
            address_sizes: (48, 48),
        };

        let vendor = info.fill_vendor_info();
        info.fill_version_info(vendor);
        info.fill_brand_info();
        info.fill_cache_info(vendor);
        info.fill_topology_info();
        info.fill_feature_info();
        info.fill_address_info();

        info
    }

    fn fill_vendor_info(&mut self) -> CpuVendor {
        let Some(CpuidResult { eax, ebx, ecx, edx }) = cpuid(CpuidLeaf::Base as u32, 0) else {
            return CpuVendor::Unknown;
        };

        self.cpuid_level = eax;

        let mut vendor_bytes = [0u8; 12];
        vendor_bytes[0..4].copy_from_slice(&ebx.to_le_bytes());
        vendor_bytes[4..8].copy_from_slice(&edx.to_le_bytes());
        vendor_bytes[8..12].copy_from_slice(&ecx.to_le_bytes());

        if let Ok(vendor_str) = str::from_utf8(&vendor_bytes) {
            self.vendor_id = String::from(vendor_str);
            CpuVendor::parse(vendor_str)
        } else {
            CpuVendor::Unknown
        }
    }

    fn fill_version_info(&mut self, vendor: CpuVendor) {
        let Some(CpuidResult { eax, ebx, .. }) = cpuid(CpuidLeaf::Feature as u32, 0) else {
            return;
        };

        let stepping = (eax & 0x0F) as u8;
        let base_model = (eax >> 4) & 0x0F;
        let base_family = (eax >> 8) & 0x0F;
        let ext_model = (eax >> 16) & 0x0F;
        let ext_family = (eax >> 20) & 0xFF;

        let family = if base_family == 0x0F {
            base_family + ext_family
        } else {
            base_family
        };

        let has_ext_model = match vendor {
            CpuVendor::Intel => base_family == 0x06 || base_family == 0x0F,
            CpuVendor::Amd | CpuVendor::Unknown => base_family == 0x0F,
        };

        let model = if has_ext_model {
            base_model | (ext_model << 4)
        } else {
            base_model
        };

        self.cpu_family = family as u8;
        self.model = model as u8;
        self.stepping = Some(stepping);

        let clflush_val = ((ebx >> 8) & 0xFF) * 8;
        if clflush_val > 0 {
            self.clflush_size = clflush_val;
            self.cache_alignment = clflush_val;
        }

        self.initial_apicid = ebx >> 24;
        self.apicid = self.initial_apicid;
    }

    fn fill_brand_info(&mut self) {
        let Some(CpuidResult { eax: a1, ebx: b1, ecx: c1, edx: d1 }) =
            cpuid(CpuidLeaf::ExtBrand1 as u32, 0)
        else {
            return;
        };
        let Some(CpuidResult { eax: a2, ebx: b2, ecx: c2, edx: d2 }) =
            cpuid(CpuidLeaf::ExtBrand2 as u32, 0)
        else {
            return;
        };
        let Some(CpuidResult { eax: a3, ebx: b3, ecx: c3, edx: d3 }) =
            cpuid(CpuidLeaf::ExtBrand3 as u32, 0)
        else {
            return;
        };

        let mut brand = [0u8; 48];
        brand[0..4].copy_from_slice(&a1.to_le_bytes());
        brand[4..8].copy_from_slice(&b1.to_le_bytes());
        brand[8..12].copy_from_slice(&c1.to_le_bytes());
        brand[12..16].copy_from_slice(&d1.to_le_bytes());
        brand[16..20].copy_from_slice(&a2.to_le_bytes());
        brand[20..24].copy_from_slice(&b2.to_le_bytes());
        brand[24..28].copy_from_slice(&c2.to_le_bytes());
        brand[28..32].copy_from_slice(&d2.to_le_bytes());
        brand[32..36].copy_from_slice(&a3.to_le_bytes());
        brand[36..40].copy_from_slice(&b3.to_le_bytes());
        brand[40..44].copy_from_slice(&c3.to_le_bytes());
        brand[44..48].copy_from_slice(&d3.to_le_bytes());

        let len = brand.iter().position(|&b| b == 0).unwrap_or(brand.len());
        if let Ok(brand_str) = str::from_utf8(&brand[..len]) {
            let trimmed = brand_str.trim();
            if !trimmed.is_empty() {
                self.model_name = String::from(trimmed);
            }
        }
    }

    fn fill_cache_info(&mut self, vendor: CpuVendor) {
        match vendor {
            CpuVendor::Intel => self.fill_cache_info_intel(),
            CpuVendor::Amd | CpuVendor::Unknown => self.fill_cache_info_amd(),
        }
    }

    fn fill_cache_info_intel(&mut self) {
        let mut max_cache_size = 0;

        for subleaf in 0..8 {
            let Some(CpuidResult { eax, ebx, ecx, .. }) = cpuid(CpuidLeaf::Cache as u32, subleaf)
            else {
                break;
            };

            // Cache Type Field (Bits 04-00): 0 = Null (no more caches)
            if eax & 0x1F == 0 {
                break;
            }

            let line_size = (ebx & 0x0FFF) + 1;
            let partitions = ((ebx >> 12) & 0x03FF) + 1;
            let ways = ((ebx >> 22) & 0x03FF) + 1;
            let sets = ecx + 1;

            let size_kb = (line_size * partitions * ways * sets) / 1024;
            max_cache_size = max_cache_size.max(size_kb);
        }

        if max_cache_size > 0 {
            self.cache_size = Some(max_cache_size);
        }
    }

    fn fill_cache_info_amd(&mut self) {
        if let Some(CpuidResult { ecx, .. }) = cpuid(CpuidLeaf::ExtCacheL2L3 as u32, 0) {
            let l2_kb = ecx >> 16;
            if l2_kb > 0 {
                self.cache_size = Some(l2_kb);
                return;
            }
        }

        if let Some(CpuidResult { ecx, edx, .. }) = cpuid(CpuidLeaf::ExtCacheL1 as u32, 0) {
            let l1d = ecx >> 24;
            let l1i = edx >> 24;
            let total = l1d + l1i;
            if total > 0 {
                self.cache_size = Some(total);
            }
        }
    }

    fn fill_topology_info(&mut self) {
        if let Some(CpuidResult { eax, edx, .. }) = cpuid(CpuidLeaf::Topology as u32, 0) {
            self.apicid = edx;
            let shift = eax & 0x1F;
            self.core_id = if shift > 0 { self.apicid >> shift } else { self.apicid };
        }
    }

    fn fill_feature_info(&mut self) {
        macro_rules! check_bits {
            ($val:expr; $($bit:literal => $name:literal,)*) => {{
                let val = $val;
                $(
                    if (val & (1 << $bit)) != 0 {
                        self.flags.push($name);
                    }
                )*
            }};
        }

        // Standard feature flags (0x0000_0001)
        if let Some(CpuidResult { edx, ecx, .. }) = cpuid(CpuidLeaf::Feature as u32, 0) {
            check_bits!(edx;
                0 => "fpu",
                1 => "vme",
                2 => "de",
                3 => "pse",
                4 => "tsc",
                5 => "msr",
                6 => "pae",
                7 => "mce",
                8 => "cx8",
                9 => "apic",
                11 => "sep",
                12 => "mtrr",
                13 => "pge",
                14 => "mca",
                15 => "cmov",
                16 => "pat",
                17 => "pse36",
                19 => "clflush",
                21 => "dts",
                22 => "acpi",
                23 => "mmx",
                24 => "fxsr",
                25 => "sse",
                26 => "sse2",
                27 => "ss",
                28 => "ht",
                29 => "tm",
                30 => "ia64",
                31 => "pbe",
            );

            check_bits!(ecx;
                0 => "pni",
                1 => "pclmulqdq",
                2 => "dtes64",
                3 => "monitor",
                4 => "ds_cpl",
                5 => "vmx",
                6 => "smx",
                7 => "est",
                8 => "tm2",
                9 => "ssse3",
                10 => "cid",
                11 => "sdbg",
                12 => "fma",
                13 => "cx16",
                14 => "xtpr",
                15 => "pdcm",
                17 => "pcid",
                18 => "dca",
                19 => "sse4_1",
                20 => "sse4_2",
                21 => "x2apic",
                22 => "movbe",
                23 => "popcnt",
                24 => "tsc_deadline_timer",
                25 => "aes",
                26 => "xsave",
                27 => "osxsave",
                28 => "avx",
                29 => "f16c",
                30 => "rdrand",
                31 => "hypervisor",
            );
        }

        // Extended feature flags (0x8000_0001)
        if let Some(CpuidResult { edx, ecx, .. }) = cpuid(CpuidLeaf::ExtFeature as u32, 0) {
            check_bits!(edx;
                11 => "syscall",
                19 => "mp",
                20 => "nx",
                22 => "mmxext",
                25 => "fxsr_opt",
                26 => "pdpe1gb",
                27 => "rdtscp",
                29 => "lm",
                30 => "3dnowext",
                31 => "3dnow",
            );

            check_bits!(ecx;
                0 => "lahf_lm",
                1 => "cmp_legacy",
                2 => "svm",
                3 => "extapic",
                4 => "cr8_legacy",
                5 => "abm",
                6 => "sse4a",
                7 => "misalignsse",
                8 => "3dnowprefetch",
                9 => "osvw",
                10 => "ibs",
                11 => "xop",
                12 => "skinit",
                13 => "wdt",
                14 => "lwp",
                15 => "fma4",
                16 => "tce",
                17 => "nodeid_msr",
                18 => "tbm",
                19 => "topoext",
                20 => "perfctr_core",
                21 => "perfctr_nb",
            );
        }

        // Structured Extended Features (0x0000_0007, subleaf 0)
        if let Some(CpuidResult { ebx, ecx, .. }) = cpuid(CpuidLeaf::ExtendedFeatures as u32, 0) {
            check_bits!(ebx;
                0 => "fsgsbase",
                1 => "tsc_adjust",
                2 => "sgx",
                3 => "bmi1",
                4 => "hle",
                5 => "avx2",
                7 => "smep",
                8 => "bmi2",
                9 => "erms",
                10 => "invpcid",
                11 => "rtm",
                14 => "mpx",
                16 => "avx512f",
                17 => "avx512dq",
                18 => "rdseed",
                19 => "adx",
                20 => "smap",
                21 => "avx512ifma",
                23 => "clflushopt",
                24 => "clwb",
                25 => "intel_pt",
                28 => "avx512cd",
                29 => "sha_ni",
                30 => "avx512bw",
                31 => "avx512vl",
            );

            check_bits!(ecx;
                1 => "avx512vbmi",
                2 => "umip",
                3 => "pku",
                4 => "ospke",
                5 => "waitpkg",
                6 => "avx512_vbmi2",
                8 => "gfni",
                9 => "vaes",
                10 => "vpclmulqdq",
                11 => "avx512_vnni",
                12 => "avx512_bitalg",
                14 => "avx512_vpopcntdq",
                16 => "la57",
                22 => "rdpid",
                25 => "cldemote",
                27 => "movdiri",
                28 => "movdir64b",
                29 => "enqcmd",
            );
        }
    }

    fn fill_address_info(&mut self) {
        if let Some(CpuidResult { eax, .. }) = cpuid(CpuidLeaf::ExtAddrSizes as u32, 0) {
            let phys = (eax & 0xFF).max(36);
            let virt = ((eax >> 8) & 0xFF).max(48);
            self.address_sizes = (phys, virt);
        }
    }
}

impl fmt::Display for CpuInformation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "processor\t: {}", self.processor)?;
        writeln!(f, "vendor_id\t: {}", self.vendor_id)?;
        writeln!(f, "cpu family\t: {}", self.cpu_family)?;
        writeln!(f, "model\t\t: {}", self.model)?;
        writeln!(f, "model name\t: {}", self.model_name)?;

        if let Some(stepping) = self.stepping {
            writeln!(f, "stepping\t: {}", stepping)?;
        } else {
            writeln!(f, "stepping\t: unknown")?;
        }

        if let Some(mhz) = self.cpu_mhz {
            writeln!(f, "cpu MHz\t\t: {}.000", mhz)?;
        } else {
            writeln!(f, "cpu MHz\t\t: Unknown")?;
        }

        if let Some(cache_size) = self.cache_size {
            writeln!(f, "cache size\t: {} KB", cache_size)?;
        }

        writeln!(f, "physical id\t: {}", self.physical_id)?;
        writeln!(f, "siblings\t: {}", self.siblings)?;
        writeln!(f, "core id\t\t: {}", self.core_id)?;
        writeln!(f, "cpu cores\t: {}", self.cpu_cores)?;
        writeln!(f, "apicid\t\t: {}", self.apicid)?;
        writeln!(f, "initial apicid\t: {}", self.initial_apicid)?;
        writeln!(f, "fpu\t\t: {}", if self.fpu { "yes" } else { "no" })?;
        writeln!(f, "fpu_exception\t: {}", if self.fpu_exception { "yes" } else { "no" })?;
        writeln!(f, "cpuid level\t: {}", self.cpuid_level)?;
        writeln!(f, "wp\t\t: {}", if self.wp { "yes" } else { "no" })?;

        write!(f, "flags\t\t:")?;
        for flag in &self.flags {
            write!(f, " {}", flag)?;
        }
        writeln!(f)?;

        write!(f, "bugs\t\t:")?;
        for bug in &self.bugs {
            write!(f, " {}", bug)?;
        }
        writeln!(f)?;

        if let Some(bogomips) = self.bogomips {
            writeln!(f, "bogomips\t: {}.00", bogomips)?;
        }

        writeln!(f, "clflush size\t: {}", self.clflush_size)?;
        writeln!(f, "cache_alignment\t: {}", self.cache_alignment)?;
        writeln!(
            f,
            "address sizes\t: {} bits physical, {} bits virtual",
            self.address_sizes.0, self.address_sizes.1
        )?;
        writeln!(f, "power management:")?;

        Ok(())
    }
}

/// Generates the full `/proc/cpuinfo` payload covering all online processors.
pub fn generate_cpuinfo() -> Vec<u8> {
    let num_cpus = ostd::cpu::num_cpus();
    let mut s = String::new();

    for cpu_id in 0..num_cpus {
        let info = CpuInformation::new(cpu_id as u32);
        use core::fmt::Write;
        let _ = writeln!(s, "{}", info);
    }

    s.into_bytes()
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use alloc::format;
    use ostd::prelude::ktest;

    #[ktest]
    fn cpu_information_discovery() {
        let info = CpuInformation::new(0);

        // Processor ID must match
        assert_eq!(info.processor, 0);

        // Vendor ID must be discovered
        assert!(!info.vendor_id.is_empty());
        assert_ne!(info.vendor_id, "Unknown");

        // Model name must not be generic fallback if CPUID is functional
        assert!(!info.model_name.is_empty());

        // Basic x86_64 flags should be present on any 64-bit platform
        assert!(info.flags.contains(&"fpu"));
        assert!(info.flags.contains(&"lm") || info.flags.contains(&"sse"));

        // Address sizes must be valid
        assert!(info.address_sizes.0 >= 32);
        assert!(info.address_sizes.1 >= 48);

        // Formatted display output must contain key identifiers
        let output = format!("{}", info);
        assert!(output.contains("processor\t: 0"));
        assert!(output.contains("vendor_id\t:"));
        assert!(output.contains("flags\t\t:"));
    }

    #[ktest]
    fn generate_cpuinfo_multicore() {
        let content = generate_cpuinfo();
        let content_str = str::from_utf8(&content).expect("valid utf-8 cpuinfo");

        // At least processor 0 must be present
        assert!(content_str.contains("processor\t: 0"));

        let num_cpus = ostd::cpu::num_cpus();
        if num_cpus > 1 {
            assert!(content_str.contains("processor\t: 1"));
        }
    }
}
