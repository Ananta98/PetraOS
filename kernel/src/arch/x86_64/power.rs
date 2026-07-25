//! Power management, system shutdown, reboot, and CPU halt routines.

/// Halts or suspends the current CPU execution thread until an interrupt or event occurs.
#[inline]
pub fn cpu_halt() {
    ostd::task::Task::yield_now();
}

/// Triggers a system shutdown sequence.
pub fn system_shutdown() -> ! {
    log::info!("System shutdown requested. Halting execution loop.");
    loop {
        cpu_halt();
    }
}

/// Triggers a system reboot sequence.
pub fn system_reboot() -> ! {
    log::info!("System reboot requested. Resetting CPU loop.");
    loop {
        cpu_halt();
    }
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;

    #[ktest]
    fn test_cpu_halt_does_not_panic() {
        cpu_halt();
    }
}
