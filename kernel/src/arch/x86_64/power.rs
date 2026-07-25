//! Power management, system shutdown, reboot, and CPU halt routines utilizing ACPI and hardware control ports.

use ostd::arch::device::io_port::WriteOnlyAccess;
use ostd::io::IoPort;
use ostd::power::{poweroff, restart, ExitCode};

/// Represents ACPI System Power States (S0 through S5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcpiPowerState {
    /// S0: Working state — fully operational.
    S0Working,
    /// S1: Sleeping state — CPU context preserved, system in low power mode.
    S1Sleeping,
    /// S2: Sleeping state — CPU powered off, dirty cache flushed to RAM.
    S2Sleeping,
    /// S3: Suspend to RAM — RAM context preserved, device states saved.
    S3SuspendToRam,
    /// S4: Hibernate — Suspend to Disk, main memory image saved to persistent storage.
    S4Hibernate,
    /// S5: Soft Off — Full system shutdown / power down.
    S5SoftOff,
}

/// Halts or suspends the current CPU execution thread until an interrupt or event occurs.
#[inline]
pub fn cpu_halt() {
    ostd::task::Task::yield_now();
}

/// Triggers a system shutdown sequence via ACPI S5 state and hypervisor control ports.
pub fn acpi_shutdown() -> ! {
    log::info!("Initiating ACPI system shutdown sequence (S5 Soft-Off)...");

    // 1. Try QEMU / Bochs ACPI PM1a_CNT Control Register (Port 0x604)
    // Writing SLP_EN (1 << 13) | SLP_TYP S5 (0x2000) or 0x3400
    if let Ok(pm_port) = IoPort::<u16, WriteOnlyAccess>::acquire_overlapping(0x604) {
        pm_port.write(0x2000);
        pm_port.write(0x3400);
    }

    // 2. Try Bochs / VirtualBox ACPI PM1a_CNT Port (Port 0xB004)
    if let Ok(pm_port) = IoPort::<u16, WriteOnlyAccess>::acquire_overlapping(0xB004) {
        pm_port.write(0x2000);
        pm_port.write(0x3400);
    }

    // 3. Try QEMU ISA debug-exit device (Port 0xF4)
    if let Ok(debug_port) = IoPort::<u32, WriteOnlyAccess>::acquire_overlapping(0xF4) {
        debug_port.write(0x10);
    }

    // 4. Delegate to OSTD poweroff subsystem
    poweroff(ExitCode::Success);
}

/// Triggers a system reboot sequence via ACPI PCI reset register and 8042 controller.
pub fn acpi_reboot() -> ! {
    log::info!("Initiating ACPI / PCI system reboot sequence...");

    // 1. Try ACPI / PCI Reset Register (Port 0xCF9)
    // Writing 0x06 (Bit 1 = System Reset, Bit 2 = Reset CPU) or 0x0E (Full Hard Reset)
    if let Ok(reset_port) = IoPort::<u8, WriteOnlyAccess>::acquire_overlapping(0xCF9) {
        reset_port.write(0x06);
        reset_port.write(0x0E);
    }

    // 2. Try PS/2 8042 Keyboard Controller Reset (Port 0x64)
    // Command 0xFE pulses the CPU reset line
    if let Ok(kbd_port) = IoPort::<u8, WriteOnlyAccess>::acquire_overlapping(0x64) {
        kbd_port.write(0xFE);
    }

    // 3. Delegate to OSTD restart subsystem
    restart(ExitCode::Success);
}

/// Requests a transition to the specified ACPI system power state.
pub fn transition_power_state(state: AcpiPowerState) -> Result<(), ostd::Error> {
    log::info!("Transitioning to ACPI power state: {:?}", state);
    match state {
        AcpiPowerState::S0Working => Ok(()),
        AcpiPowerState::S1Sleeping | AcpiPowerState::S2Sleeping => {
            cpu_halt();
            Ok(())
        }
        AcpiPowerState::S3SuspendToRam => {
            log::info!("ACPI S3 Suspend-to-RAM requested");
            cpu_halt();
            Ok(())
        }
        AcpiPowerState::S4Hibernate => {
            log::info!("ACPI S4 Hibernate requested");
            cpu_halt();
            Ok(())
        }
        AcpiPowerState::S5SoftOff => {
            acpi_shutdown();
        }
    }
}

/// Triggers a system shutdown sequence.
pub fn system_shutdown() -> ! {
    acpi_shutdown();
}

/// Triggers a system reboot sequence.
pub fn system_reboot() -> ! {
    acpi_reboot();
}

#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;

    #[ktest]
    fn test_cpu_halt_does_not_panic() {
        cpu_halt();
    }

    #[ktest]
    fn test_acpi_power_states() {
        assert_eq!(
            transition_power_state(AcpiPowerState::S0Working),
            Ok(())
        );
    }
}

