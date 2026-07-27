use super::arch;
use super::device::{self, PciDevice};
/// PCI Bus Enumeration
///
/// Scans the PCI bus hierarchy to discover all connected devices.
/// Uses brute-force enumeration: iterates all buses (0-255),
/// devices (0-31), and functions (0-7).
use alloc::vec::Vec;

/// Enumerate all PCI devices on the system.
///
/// Performs a full bus scan across all 256 buses, 32 devices per bus,
/// and up to 8 functions per device. Multi-function devices are detected
/// by checking bit 7 of the header type register on function 0.
pub fn enumerate() -> Vec<PciDevice> {
    let mut devices = Vec::new();

    for bus in 0..=255u16 {
        for dev in 0..32u8 {
            scan_device(&mut devices, bus as u8, dev);
        }
    }

    devices
}

/// Find all PCI devices matching a given class and subclass code.
///
/// Useful for locating devices by category (e.g., mass storage controllers,
/// network controllers) without knowing specific vendor/device IDs.
pub fn find_devices_by_class(class: u8, subclass: u8) -> Vec<PciDevice> {
    enumerate()
        .into_iter()
        .filter(|d| d.class_code == class && d.subclass == subclass)
        .collect()
}

/// Find the first PCI device matching a specific vendor and device ID.
pub fn find_device(vendor_id: u16, device_id: u16) -> Option<PciDevice> {
    enumerate()
        .into_iter()
        .find(|d| d.vendor_id == vendor_id && d.device_id == device_id)
}

/// Scan a single device slot for present functions.
fn scan_device(devices: &mut Vec<PciDevice>, bus: u8, dev: u8) {
    // Check function 0 first
    let id = arch::config_read_u32(bus, dev, 0, 0x00);
    if (id & 0xFFFF) == 0xFFFF {
        return;
    }

    // Probe function 0
    if let Some(dev0) = device::probe(bus, dev, 0) {
        let is_multi_func = (arch::config_read_u8(bus, dev, 0, 0x0E) & 0x80) != 0;
        devices.push(dev0);

        // If multi-function, check functions 1-7
        if is_multi_func {
            for func in 1..8u8 {
                if let Some(devn) = device::probe(bus, dev, func) {
                    devices.push(devn);
                }
            }
        }
    }
}
