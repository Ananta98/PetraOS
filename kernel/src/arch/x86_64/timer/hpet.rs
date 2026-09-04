//! High Precision Event Timer (HPET) Driver for x86_64
//!
//! Provides high-resolution timing, elapsed time measurements, and microsecond/millisecond busy-wait delays.

use crate::arch::acpi;
use crate::mm::map_mmio;
use crate::sync::Mutex;

pub const HPET_DEFAULT_PHYS_BASE: u64 = 0xFED0_0000;

// HPET Register Offsets (relative to HPET MMIO base address)
const REG_CAP_ID: usize = 0x00; // General Capabilities and ID (64-bit)
const REG_CONFIG: usize = 0x10; // General Configuration (64-bit)
const REG_INT_STATUS: usize = 0x20; // General Interrupt Status (64-bit)
const REG_COUNTER: usize = 0xF0; // Main Counter Value (64-bit)

// Configuration register flags
const CONFIG_ENABLE: u64 = 1 << 0; // Overall Enable (starts main counter)
const CONFIG_LEG_RT: u64 = 1 << 1; // Legacy Replacement Route

/// High Precision Event Timer Hardware Abstraction.
#[derive(Debug)]
pub struct Hpet {
    phys_base: u64,
    virt_base: *mut u8,
    counter_clk_period_fs: u64, // Femtoseconds per counter tick (10^-15 sec)
    num_timers: usize,
    is_64bit: bool,
}

// SAFETY: Hpet stores a raw MMIO pointer that is safe to share across CPU cores.
unsafe impl Send for Hpet {}
unsafe impl Sync for Hpet {}

pub static HPET: Mutex<Option<Hpet>> = Mutex::new(None);

impl Hpet {
    /// Read a 64-bit register at the specified byte offset from HPET MMIO base.
    #[inline]
    unsafe fn read_reg(&self, offset: usize) -> u64 {
        let ptr = unsafe { self.virt_base.add(offset) } as *const u64;
        unsafe { core::ptr::read_volatile(ptr) }
    }

    /// Write a 64-bit register at the specified byte offset from HPET MMIO base.
    #[inline]
    unsafe fn write_reg(&self, offset: usize, val: u64) {
        let ptr = unsafe { self.virt_base.add(offset) } as *mut u64;
        unsafe { core::ptr::write_volatile(ptr, val) };
    }

    /// Read current value of the HPET main counter.
    #[inline]
    pub fn read_counter(&self) -> u64 {
        unsafe { self.read_reg(REG_COUNTER) }
    }

    /// Returns clock tick period in femtoseconds (10^-15 s).
    #[inline]
    pub fn period_fs(&self) -> u64 {
        self.counter_clk_period_fs
    }

    /// Convert tick count into elapsed nanoseconds (1 ns = 1,000,000 fs).
    #[inline]
    pub fn ticks_to_ns(&self, ticks: u64) -> u64 {
        (ticks as u128 * self.counter_clk_period_fs as u128 / 1_000_000) as u64
    }

    /// Convert nanoseconds into required HPET counter ticks.
    #[inline]
    pub fn ns_to_ticks(&self, ns: u64) -> u64 {
        if self.counter_clk_period_fs == 0 {
            return 0;
        }
        (ns as u128 * 1_000_000 / self.counter_clk_period_fs as u128) as u64
    }
}

/// Parse ACPI tables to locate the HPET base physical address.
pub fn parse_hpet_base() -> Option<u64> {
    let child_table = acpi::find_table(b"HPET")?;
    let hhdm = crate::mm::hhdm_offset();

    // ACPI HPET table layout:
    // Header: 36 bytes
    // Hardware Block ID: 4 bytes (offset 36)
    // Base Address GAS structure: 12 bytes (offset 40)
    // Physical Address field is at offset 44 (GAS address field)
    let base_addr_ptr =
        (child_table.length() >= 52).then(|| (child_table.phys_addr() + hhdm + 44) as *const u64)?;

    // SAFETY: HPET table is mapped and base_addr_ptr is within the table bounds.
    let phys_addr = unsafe { core::ptr::read_unaligned(base_addr_ptr) };
    if phys_addr != 0 {
        Some(phys_addr)
    } else {
        None
    }
}

/// Initialize High Precision Event Timer hardware.
pub fn init() {
    let phys_base = parse_hpet_base().unwrap_or(HPET_DEFAULT_PHYS_BASE);

    // Map HPET MMIO page
    map_mmio(phys_base, 4096);
    let hhdm = crate::mm::hhdm_offset();
    let virt_base = (phys_base + hhdm) as *mut u8;

    let mut hpet = Hpet {
        phys_base,
        virt_base,
        counter_clk_period_fs: 0,
        num_timers: 0,
        is_64bit: false,
    };

    // Read General Capabilities and ID
    let cap_id = unsafe { hpet.read_reg(REG_CAP_ID) };
    let period_fs = (cap_id >> 32) as u64;
    let num_timers = (((cap_id >> 8) & 0x1F) + 1) as usize;
    let is_64bit = (cap_id & (1 << 13)) != 0;

    hpet.counter_clk_period_fs = period_fs;
    hpet.num_timers = num_timers;
    hpet.is_64bit = is_64bit;

    // Enable main counter in General Configuration
    unsafe {
        let cfg = hpet.read_reg(REG_CONFIG);
        hpet.write_reg(REG_CONFIG, cfg | CONFIG_ENABLE);
    }

    let freq_mhz = if period_fs > 0 {
        1_000_000_000 / period_fs
    } else {
        0
    };

    log::info!(
        "HPET: Base={:#x}, Period={} fs ({} MHz), Timers={}, 64-bit counter={}",
        phys_base,
        period_fs,
        freq_mhz,
        num_timers,
        is_64bit
    );

    *HPET.lock() = Some(hpet);
}

/// Read current main counter value.
pub fn read_counter() -> u64 {
    HPET.lock().as_ref().map(|h| h.read_counter()).unwrap_or(0)
}

/// Read elapsed nanoseconds since boot from HPET.
pub fn elapsed_ns() -> u64 {
    let guard = HPET.lock();
    if let Some(ref hpet) = *guard {
        hpet.ticks_to_ns(hpet.read_counter())
    } else {
        0
    }
}

/// High-precision busy-wait sleep for `ns` nanoseconds using HPET counter.
pub fn sleep_ns(ns: u64) {
    let guard = HPET.lock();
    if let Some(ref hpet) = *guard {
        let start_counter = hpet.read_counter();
        let target_ticks = hpet.ns_to_ticks(ns);
        drop(guard);

        while read_counter().wrapping_sub(start_counter) < target_ticks {
            core::hint::spin_loop();
        }
    }
}

/// High-precision busy-wait sleep for `us` microseconds using HPET counter.
pub fn sleep_us(us: u64) {
    sleep_ns(us.saturating_mul(1_000));
}

/// High-precision busy-wait sleep for `ms` milliseconds using HPET counter.
pub fn sleep_ms(ms: u64) {
    sleep_ns(ms.saturating_mul(1_000_000));
}
