use crate::drivers::{Device, Driver, DeviceType};
use super::Timer;

/// Time Stamp Counter (TSC) timer implementation.
pub struct Tsc;

impl Tsc {
    /// Create a new TSC driver instance.
    pub fn new() -> Self {
        Self
    }
}

impl Device for Tsc {
    fn name(&self) -> &str {
        "tsc"
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Char
    }
}

impl Driver for Tsc {
    fn name(&self) -> &str {
        "tsc"
    }

    fn probe(&self) -> Result<(), ostd::Error> {
        if ostd::arch::tsc_freq() == 0 {
            Err(ostd::Error::InvalidArgs)
        } else {
            Ok(())
        }
    }
}

impl Timer for Tsc {
    fn current_time_ns(&self) -> u64 {
        let cycles = ostd::arch::read_tsc();
        let freq = ostd::arch::tsc_freq();
        if freq == 0 {
            0
        } else {
            ((cycles as u128 * 1_000_000_000) / freq as u128) as u64
        }
    }
}
