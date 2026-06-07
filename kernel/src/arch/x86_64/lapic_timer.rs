//! LAPIC Timer driver with PIT-based calibration.
//!
//! The LAPIC timer is a per-CPU timer integrated into each Local APIC.
//! It can operate in periodic or one-shot mode and is commonly used
//! as the scheduler tick source.
//!
//! Calibration is performed using the PIT (Programmable Interval Timer)
//! channel 2 as a reference clock to determine the LAPIC timer frequency.

use super::lapic::LocalApic;
use super::ports::Ports;

/// The IDT vector number used for LAPIC timer interrupts.
pub const TIMER_VECTOR: u8 = 48;

/// LVT Timer mode bits.
const LVT_TIMER_PERIODIC: u32 = 1 << 17;
const LVT_TIMER_MASKED: u32 = 1 << 16;

/// Timer divide configuration values.
/// Divide by 16 provides a good balance between resolution and range.
const TIMER_DIVIDE_BY_16: u32 = 0x03;

/// PIT constants for calibration.
const PIT_CHANNEL2_PORT: u16 = 0x42;
const PIT_COMMAND_PORT: u16 = 0x43;
const PIT_GATE_PORT: u16 = 0x61;

/// PIT base frequency in Hz.
const PIT_FREQUENCY: u32 = 1_193_182;

/// Calibration duration in milliseconds.
const CALIBRATION_MS: u32 = 10;

/// Represents the LAPIC timer associated with a Local APIC.
///
/// The timer must be calibrated before use to determine the correct
/// tick count for the desired interrupt frequency.
pub struct LapicTimer {
    /// Calibrated ticks per millisecond (after divider).
    ticks_per_ms: u32,
}

impl LapicTimer {
    /// Calibrate the LAPIC timer using the PIT as a reference clock.
    ///
    /// This performs a busy-wait calibration by:
    /// 1. Configuring PIT channel 2 for a known duration
    /// 2. Running the LAPIC timer in one-shot mode simultaneously
    /// 3. Measuring elapsed LAPIC ticks to compute ticks-per-millisecond
    pub fn calibrate(lapic: &LocalApic) -> Self {
        // Set divide configuration to divide-by-16
        lapic.write_timer_divide_config(TIMER_DIVIDE_BY_16);

        // Configure PIT channel 2 for a one-shot countdown
        let pit_count = (PIT_FREQUENCY / 1000) * CALIBRATION_MS;

        // SAFETY: These I/O port writes configure PIT channel 2 for calibration.
        // Port 0x61 controls the speaker/PIT gate. We disable the speaker output
        // and enable the PIT gate to use channel 2 as a countdown reference.
        unsafe {
            // Disable speaker output, enable PIT channel 2 gate
            let gate = Ports::inb(PIT_GATE_PORT);
            Ports::outb(PIT_GATE_PORT, (gate & 0xFD) | 0x01);

            // PIT command: channel 2, lobyte/hibyte, mode 0 (one-shot), binary
            Ports::outb(PIT_COMMAND_PORT, 0b10110000);

            // Load the countdown value
            Ports::outb(PIT_CHANNEL2_PORT, (pit_count & 0xFF) as u8);
            Ports::outb(PIT_CHANNEL2_PORT, ((pit_count >> 8) & 0xFF) as u8);
        }

        // Start LAPIC timer with max initial count (masked so no interrupt fires)
        lapic.write_lvt_timer(LVT_TIMER_MASKED);
        lapic.write_timer_initial_count(0xFFFF_FFFF);

        // SAFETY: Reading PIT gate port bit 5 to poll for countdown completion.
        // Bit 5 of port 0x61 indicates PIT channel 2 output status.
        unsafe {
            // Reset the PIT channel 2 gate to start the countdown
            let gate = Ports::inb(PIT_GATE_PORT);
            Ports::outb(PIT_GATE_PORT, gate & 0xFE);
            Ports::outb(PIT_GATE_PORT, gate | 0x01);

            // Wait for PIT channel 2 output to go high (bit 5)
            while (Ports::inb(PIT_GATE_PORT) & 0x20) == 0 {
                core::hint::spin_loop();
            }
        }

        // Read how many LAPIC ticks elapsed during the PIT countdown
        let elapsed = 0xFFFF_FFFF - lapic.read_timer_current_count();

        // Stop the timer
        lapic.write_timer_initial_count(0);

        let ticks_per_ms = elapsed / CALIBRATION_MS;

        log::info!(
            "LAPIC timer calibrated: {} ticks/ms (elapsed {} ticks in {}ms).",
            ticks_per_ms,
            elapsed,
            CALIBRATION_MS
        );

        Self { ticks_per_ms }
    }

    /// Start the LAPIC timer in periodic mode at the given frequency.
    ///
    /// # Arguments
    /// * `lapic` — Reference to the Local APIC owning this timer
    /// * `frequency_hz` — Desired interrupt frequency in Hz
    pub fn start_periodic(&self, lapic: &LocalApic, frequency_hz: u32) {
        if frequency_hz == 0 || self.ticks_per_ms == 0 {
            log::warn!("LAPIC timer: invalid frequency or uncalibrated timer.");
            return;
        }

        // Calculate initial count: ticks_per_ms * 1000 / frequency_hz
        let ticks_per_second = self.ticks_per_ms as u64 * 1000;
        let initial_count = (ticks_per_second / frequency_hz as u64) as u32;

        // Set divide configuration
        lapic.write_timer_divide_config(TIMER_DIVIDE_BY_16);

        // Configure LVT Timer: periodic mode, unmasked, with our vector
        lapic.write_lvt_timer(LVT_TIMER_PERIODIC | TIMER_VECTOR as u32);

        // Set the initial count to start the timer
        lapic.write_timer_initial_count(initial_count);

        log::info!(
            "LAPIC timer started: periodic mode at {}Hz (initial count: {}).",
            frequency_hz,
            initial_count
        );
    }

    /// Stop the LAPIC timer by masking its LVT entry and zeroing the count.
    pub fn stop(&self, lapic: &LocalApic) {
        lapic.write_lvt_timer(LVT_TIMER_MASKED);
        lapic.write_timer_initial_count(0);
    }
}
